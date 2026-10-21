//! 型付き HIR から、前向きの辺だけを持つブロックの列への変換 (docs/spec/core-ir.md)。式の値の渡し先 (出口) と、
//! 条件の分かれ方という制御の骨組みをここに置く。ブロックの組み立ては crate の `builder.rs`、式ごとの変換は `expr.rs`、
//! パターンの決定木と case-of-case は `pattern.rs`、関数の表と包む関数は `program.rs`、型から決まる Repr は
//! `types.rs`、導出した instance とタプルの instance の生成器は `derive.rs`、REPL の式の入力の値を書く表示関数の生成器は `display.rs` にある。

mod derive;
mod display;
mod expr;
mod instances;
mod names;
mod pattern;
mod program;
mod types;

use std::collections::{BTreeSet, HashMap, HashSet};

use eml_diagnostics::{FileId, SourceFiles};
use eml_extern::Extern;
use eml_hir::{
    Body, Closure, ExprId, ExprKind, FunctionId, FunctionKind, InputKind, LocalId, MatchArm, PatId,
    PatKind, Program as HirProgram, Res, Stmt as HirStmt, ValueItem,
};
use eml_types::{
    BodyTypes, InputDisplay, Lin, RowYield, TypeId, TypeKind, TypeStore, TypedProgram,
};
use la_arena::ArenaMap;

use crate::{CoreFn, Program, Rhs, Term};
use eml_lir::{Atom, Case, CasePattern, FALSE, FnIdx, Loc, Mark, Repr, Role, TRUE, VarId};

use crate::builder::{FnBuilder, Label};
use expr::extern_row;
use instances::{InstanceKind, Part, Target, wrapper_name};
use names::NameTable;
use pattern::{Known, MatchCtx, Occ, Scrutinee, destructures};
use program::{EntryAfter, ProgramBuilder, ShowTarget, Shown, effect_table, recursive_types};
use types::{StructTypes, mark, named, split_arrows};

// extern の表の行と std の宣言を照らし合わせる結合テスト (tests/externs.rs) が、translate と同じ規則で型の Repr を決める。
// extern の行に構造体は現れないので、構造体にする型も箱の位置の `obj` として読む
pub use types::boxed_repr;
pub(crate) use types::tuple_struct;

/// 節の `k` の変換の形 (docs/spec/core-ir.md)。
#[derive(Clone, Copy, PartialEq, Eq)]
enum ContinuationForm {
    /// `k` を生の継続のまま持ち、呼び出しを `Call::Resume` にする。`k` を捕まえた入れ子の関数でも生の継続のままである。
    Direct,
    /// 節の入口で、生の継続を `cont$` か `cont$state` のクロージャに包み、それを `k` とする。
    Wrapped,
}

/// 本体の中の局所変数ごとの参照の数。`continuation_forms` と同じく arena の式を全部見て数えるので、入れ子のラムダや
/// handle の中の参照も、別の制御の道の参照も同じ数に入る。戻らない呼び出しの前の暗黙の `drop`
/// (`BodyTypes::implicit_drops`) も1つの参照に数える。数えないと、別の枝の `match` だけが参照する `con` の変数を
/// 決定木が開き、`drop` が残す `con` とフィールドを二重に持つためである (docs/spec/core-ir.md の「translate が決める印」と「戻らない呼び出しの前の `drop`」)。
fn references(
    body: &Body,
    implicit_drops: &HashMap<ExprId, Vec<LocalId>>,
) -> ArenaMap<LocalId, u32> {
    let mut counts = ArenaMap::default();
    for (_, expr) in body.exprs.iter() {
        if let ExprKind::Path(Res::Local(local)) = expr.kind {
            *counts.entry(local).or_insert(0) += 1;
        }
    }
    for &local in implicit_drops.values().flatten() {
        *counts.entry(local).or_insert(0) += 1;
    }
    counts
}

/// 本体の中の節の `k` ごとの変換の形。`k` の出現がすべて、引数をそろえた呼び出しの呼ばれる式か `drop` の値なら
/// 直接の形にする。式の種類ごとに子をたどらず、arena の式を全部見て出現を数える。式の種類が増えて直接の呼び出しと
/// して数え漏らした出現があっても、包む形になるだけで、生の継続を関数の値として `apply` することはない。`k` の
/// 局所変数は本体の中で一意なので、入れ子のラムダや handle の中の出現も同じ数に入る。
fn continuation_forms(body: &Body) -> ArenaMap<LocalId, ContinuationForm> {
    // 出現の数と、そのうち直接の形で扱える出現の数
    let mut counts: ArenaMap<LocalId, (usize, usize)> = body
        .continuations
        .iter()
        .map(|(local, _)| (local, (0, 0)))
        .collect();
    let continuation = |expr: ExprId| match body.exprs[expr].kind {
        ExprKind::Path(Res::Local(local)) => {
            body.continuations.get(local).map(|&arity| (local, arity))
        }
        _ => None,
    };
    for (_, expr) in body.exprs.iter() {
        // `k` を置いた位置と、そこで渡す引数の数。`drop` は引数の数によらない
        let (operand, args) = match &expr.kind {
            ExprKind::Path(Res::Local(local)) => {
                if let Some((uses, _)) = counts.get_mut(*local) {
                    *uses += 1;
                }
                continue;
            }
            ExprKind::Call { callee, args } => (*callee, Some(args.len())),
            ExprKind::Drop(value) => (*value, None),
            _ => continue,
        };
        if let Some((local, arity)) = continuation(operand)
            && args.is_none_or(|args| args >= arity)
        {
            counts[local].1 += 1;
        }
    }
    counts
        .iter()
        .map(|(local, &(uses, direct))| {
            let form = if uses == direct {
                ContinuationForm::Direct
            } else {
                ContinuationForm::Wrapped
            };
            (local, form)
        })
        .collect()
}

/// 本体の中で関数を作る式の番号。`$lambdaN`、`$handleN`、`$externN` の N である。`var` の範囲の `$varN` の N は、
/// 本体の中の変数の番号 (`eml_hir::Body::vars` の順) である。変換の順に依らず、同じ instance
/// なら同じ名前になるように、本体を変換する前に式の ID の順で振る (docs/spec/core-ir.md)。extern で結んだメソッドの
/// 参照は行き先が instance ごとに違うので、`$externN` の番号は同じ本体でも instance ごとに変わりうる。
struct Numbering {
    /// ローカル関数を定義するラムダは数えない。
    lambdas: ArenaMap<ExprId, u32>,
    handlers: ArenaMap<ExprId, u32>,
    /// 値として使う extern の参照 (引数をそろえて呼ぶ位置にない参照と、部分適用の呼ばれる式)。extern で結んだ
    /// メソッドの参照を含む。
    externs: ArenaMap<ExprId, u32>,
    /// ローカル関数を定義するラムダから、持ち上げた関数の名前の接尾辞 (`let$go`、`let$go#2`) への表
    /// (docs/spec/core-ir.md の「ローカル関数の持ち上げ」)。
    local_functions: ArenaMap<ExprId, String>,
}

/// `targets` は、この instance の本体の中の参照の行き先である。
fn numbering(hir: &HirProgram, body: &Body, targets: &ArenaMap<ExprId, Target>) -> Numbering {
    let extern_of = |expr: ExprId| match body.exprs[expr].kind {
        ExprKind::Path(Res::Item(ValueItem::Function(function))) => extern_row(hir, function),
        ExprKind::Path(Res::Item(ValueItem::Method(_))) => match targets.get(expr) {
            Some(Target::Extern(row)) => Some(*row),
            Some(Target::Function(_)) | None => None,
        },
        _ => None,
    };
    // 引数をそろえて呼ぶ extern の参照は `Rhs::Extern` になり、包む関数を作らない
    let saturated: HashSet<ExprId> = body
        .exprs
        .iter()
        .filter_map(|(_, expr)| match &expr.kind {
            ExprKind::Call { callee, args } => {
                let row = extern_of(*callee)?;
                (args.len() >= row.row().params.len()).then_some(*callee)
            }
            _ => None,
        })
        .collect();
    let mut lambdas = Vec::new();
    let mut handlers = Vec::new();
    let mut externs = Vec::new();
    for (id, expr) in body.exprs.iter() {
        match &expr.kind {
            ExprKind::Lambda(_) if !body.defining.contains_idx(id) => lambdas.push(id),
            ExprKind::Handle { .. } => handlers.push(id),
            ExprKind::Path(Res::Item(_)) if extern_of(id).is_some() && !saturated.contains(&id) => {
                externs.push(id)
            }
            _ => {}
        }
    }
    // 同じ名前の2つ目から `#2`、`#3` を付ける。`LocalId` の順は HIR の順である
    let mut seen: HashMap<&str, u32> = HashMap::new();
    let local_functions = body
        .local_functions
        .iter()
        .map(|(local, &lambda)| {
            let name = body.locals[local].name.as_str();
            let count = seen.entry(name).or_insert(0);
            *count += 1;
            let suffix = match *count {
                1 => format!("let${name}"),
                n => format!("let${name}#{n}"),
            };
            (lambda, suffix)
        })
        .collect();
    let number = |ids: Vec<ExprId>| ids.into_iter().zip(0..).collect();
    Numbering {
        lambdas: number(lambdas),
        handlers: number(handlers),
        externs: number(externs),
        local_functions,
    }
}

/// 持ち上げたローカル関数。`captured` は持ち上げの捕まえる変数で、`LocalId` の順に並び、ローカル関数を含まない。
/// 再帰する組の関数は同じ `captured` を持つ (docs/spec/core-ir.md の「ローカル関数の持ち上げ」)。
struct Lifted {
    target: FnIdx,
    captured: Vec<LocalId>,
}

/// HIR の捕まえる変数 `captured` のうちローカル関数を、その持ち上げの捕まえる変数に置き換える。持ち上げの捕まえる
/// 変数はローカル関数を含まないので、置き換えは1段で済む。
fn lifted_captures(
    lifted: &ArenaMap<LocalId, Lifted>,
    captured: impl IntoIterator<Item = LocalId>,
) -> Vec<LocalId> {
    let mut locals = BTreeSet::new();
    for local in captured {
        match lifted.get(local) {
            Some(function) => locals.extend(function.captured.iter().copied()),
            None => {
                locals.insert(local);
            }
        }
    }
    locals.into_iter().collect()
}

/// 本体のローカル関数の、関数の番号と持ち上げの捕まえる変数の表。自分と組の相手への参照と、後で変換する入れ子の関数
/// からの参照のために、どのローカル関数を変換するより前に作る。再帰する組の関数は、メンバーが捕まえる変数の和を共通の
/// 追加の引数として受け取る。そのため、組の中の呼び出しは、受け取った追加の引数をそのまま相手に渡せる。組の外の関数で
/// 参照できるのは前の組の関数だけで、`local_groups` は外側の組を入れ子の組より先に並べるので、組の順に1回ずつ求めれば
/// よい (docs/spec/core-ir.md の「ローカル関数の持ち上げ」)。組の名前は本体より先に束縛するので、関数の番号を取る
/// 定義の順は `LocalId` の順と同じである。
fn lifted_functions(body: &Body, program: &mut ProgramBuilder) -> ArenaMap<LocalId, Lifted> {
    let mut lifted = ArenaMap::default();
    for group in &body.local_groups {
        // 再帰しない組のメンバーは互いを参照しないので、捕まえる変数を共有しなくてよい。共有すると、あるメンバーが
        // 捕まえた `Lin` の値を、ほかのメンバーの呼び出しも受け渡して複製してしまう。再帰する組が捕まえるのは `Unr`
        // の値だけなので、共有しても複製してよい (docs/spec/linearity.md の「基本の規則」)
        let sharing: Vec<&[LocalId]> = if group.recursive {
            vec![&group.members]
        } else {
            group.members.iter().map(std::slice::from_ref).collect()
        };
        for members in sharing {
            let hir_captured = members
                .iter()
                .flat_map(|&member| {
                    body.closure_captures(local_closure(body, body.local_functions[member]))
                })
                .filter(|captured| !members.contains(captured));
            let captured = lifted_captures(&lifted, hir_captured);
            for &member in members {
                let closure = local_closure(body, body.local_functions[member]);
                let target = program.reserve(captured.len() + closure.params.len());
                lifted.insert(
                    member,
                    Lifted {
                        target,
                        captured: captured.clone(),
                    },
                );
            }
        }
    }
    lifted
}

/// ローカル関数を定義するラムダの中身。
fn local_closure(body: &Body, lambda: ExprId) -> &Closure {
    match &body.exprs[lambda].kind {
        ExprKind::Lambda(closure) => closure,
        _ => unreachable!("a local function is defined by a lambda"),
    }
}

/// 表の各ローカル関数を、外側の instance の名前を根にした内部の関数に変換する。
fn lower_local_functions(ctx: BodyCtx<'_>, program: &mut ProgramBuilder) {
    for (local, function) in ctx.lifted.iter() {
        let lambda = ctx.body.local_functions[local];
        let closure = local_closure(ctx.body, lambda);
        let name = format!(
            "{}${}",
            ctx.root_name, ctx.numbering.local_functions[lambda]
        );
        let lambda_ty = ctx.types.exprs[lambda];
        if cfg!(debug_assertions) {
            let captured: Vec<TypeId> = ctx
                .body
                .closure_captures(closure)
                .iter()
                .map(|&captured| ctx.types.locals[captured])
                .collect();
            ctx.check_captures(lambda_ty, &captured, "local function");
        }
        let (params, ret) = ctx.lambda_signature(program, lambda_ty, &closure.params);
        let yields = ctx.resolve(
            *ctx.types
                .lambda_yields
                .get(lambda)
                .expect("every lambda has its row classified"),
        );
        let captured = ctx.typed_captures(program, &function.captured);
        // ローカル関数は直接呼ばれるために生まれるので、内部の関数でない。値として使う所も、型どおりの ABI のまま
        // 同じ関数を指す (docs/spec/lir.md の「関数の ABI」)
        let core = FnLowering::new(ctx, program, yields).lower(
            &name,
            false,
            &captured,
            &params,
            closure.body,
            ret,
        );
        program.finish(function.target, core);
    }
}

/// 誤りのない型付き HIR を Core IR にする。RC の命令と末尾呼び出しは、LIR の Perceus と縮約が作る。
pub(crate) fn translate(
    hir: &HirProgram,
    typed: &TypedProgram,
    entry: FunctionId,
    files: &SourceFiles,
) -> Program {
    let mut instances = instances::collect(hir, typed, entry);
    let struct_types = StructTypes::new(hir, typed, &mut instances.store, recursive_types(hir));
    let mut builder = ProgramBuilder::new(typed, struct_types);
    let store = &instances.store;
    let indices: Vec<FnIdx> = instances
        .list
        .iter()
        .map(|instance| match &instance.kind {
            InstanceKind::Function(function) => {
                let body = hir
                    .body(function.function)
                    .expect("a program without errors has an equation for every function");
                builder.reserve(body.params.len())
            }
            InstanceKind::Generated(generated) => {
                builder.reserve(generated.generated.method.arity())
            }
            // `(acc, budget, depth, prec, x)` (docs/spec/core-ir.md の「表示関数」)
            InstanceKind::Display(_) => builder.reserve(5),
        })
        .collect();
    for (instance, &index) in instances.list.iter().zip(&indices) {
        let function = match &instance.kind {
            InstanceKind::Function(function) => function,
            InstanceKind::Generated(generated) => {
                let core = derive::generate(
                    hir,
                    store,
                    &mut builder,
                    &indices,
                    &instance.name,
                    generated,
                );
                builder.finish(index, core);
                continue;
            }
            InstanceKind::Display(display) => {
                let core =
                    display::generate(hir, store, &mut builder, &indices, &instance.name, display);
                builder.finish(index, core);
                continue;
            }
        };
        let id = function.function;
        let body = hir.body(id).expect("checked above");
        let (param_types, ret) = split_arrows(store, function.signature, body.params.len());
        let params: Vec<Param> = body
            .params
            .iter()
            .zip(&param_types)
            .map(|(&pat, &ty)| {
                (
                    Some(pat),
                    builder.repr(hir, store, ty),
                    mark(store, ty, hir),
                )
            })
            .collect();
        let forms = continuation_forms(body);
        let numbers = numbering(hir, body, &function.targets);
        let lifted = lifted_functions(body, &mut builder);
        let types = function
            .types
            .as_deref()
            .unwrap_or_else(|| typed.bodies.get(id).expect("every body is type-checked"));
        let references = references(body, &types.implicit_drops);
        let ret = (builder.repr(hir, store, ret), mark(store, ret, hir));
        let ctx = BodyCtx {
            hir,
            body,
            store,
            types,
            bits: &function.bits,
            targets: &function.targets,
            indices: &indices,
            root_name: &instance.name,
            names: &instances.names,
            numbering: &numbers,
            continuation_forms: &forms,
            references: &references,
            lifted: &lifted,
            source: Source {
                files,
                file_id: hir.modules[id.module].file,
            },
        };
        lower_local_functions(ctx, &mut builder);
        let yields = ctx.resolve(types.body_yield);
        let core = FnLowering::new(ctx, &mut builder, yields).lower(
            &instance.name,
            false,
            &[],
            &params,
            body.root,
            ret,
        );
        builder.finish(index, core);
    }
    let InstanceKind::Function(entry_instance) = &instances.list[instances.entry.0].kind else {
        unreachable!("the entry is a function of the program")
    };
    // 入口の関数とその中の `()` の呼び出しの印は、宣言の型の最初の矢印から決める (docs/spec/core-ir.md の
    // 「`yields` の印」)
    let first_arrow = typed
        .bodies
        .get(entry)
        .expect("every body is type-checked")
        .first_arrow_yield;
    let after = entry_after(hir, typed, entry, &instances, &indices);
    let entry_fn = builder.entry(
        hir,
        store,
        indices[instances.entry.0],
        entry,
        entry_instance.signature,
        instances::resolve(first_arrow, &entry_instance.bits),
        after,
    );
    Program {
        functions: builder
            .functions
            .into_iter()
            .map(|function| function.expect("every reserved function is lowered"))
            .collect(),
        entry: entry_fn,
        strings: builder.strings.values,
        layouts: builder.layouts,
        structs: builder.structs,
        effects: effect_table(hir, store, typed),
        files: builder.files.values,
    }
}

/// 入口の関数が入口の結果をどう扱うか。REPL の入力なら、`let` は根に置き、式は値を書いた文字列を返す
/// (docs/spec/core-ir.md の「REPL の入力」)。
fn entry_after(
    hir: &HirProgram,
    typed: &TypedProgram,
    entry: FunctionId,
    instances: &instances::Instances,
    indices: &[FnIdx],
) -> EntryAfter {
    let FunctionKind::Input(kind) = hir[entry].kind else {
        return EntryAfter::Return;
    };
    match (kind, &typed.inputs[entry].display) {
        (InputKind::Let(binding), _) => EntryAfter::Store(hir[binding].root),
        (InputKind::Expr, InputDisplay::Unit) => EntryAfter::Return,
        (InputKind::Expr, InputDisplay::Value | InputDisplay::Typed(_)) => {
            let shown = match instances.display.expect("a shown input has a display") {
                Part::Display(instance) => Shown::Display(indices[instance.0]),
                Part::Show(Target::Function(instance)) => {
                    Shown::Show(ShowTarget::Function(indices[instance.0]))
                }
                Part::Show(Target::Extern(row)) => Shown::Show(ShowTarget::Extern(row)),
                Part::Function => Shown::Function,
                Part::Extern => Shown::Extern,
                Part::Ellipsis => unreachable!("the value of an input is never cut"),
            };
            EntryAfter::Shown(shown)
        }
        (InputKind::Query, _) => unreachable!("a query is checked but never lowered"),
    }
}

/// 関数の引数のパターンと、受ける値の Repr と印。パターンが `None` なら名前のない引数 (handle の本体が受ける `()`) である。
type Param = (Option<PatId>, Repr, Mark);

/// 式の値の渡し先 (docs/spec/core-ir.md)。
#[derive(Clone, Copy)]
enum Exit {
    Return,
    /// 値を引数にしてラベルへ向かう。値の `if` の続きに使う。
    Jump(Label),
    /// 値をそのまま調べる。値が分かれば、作らずに行き先を選ぶ。
    Scrutinize(CtxId),
}

#[derive(Clone, Copy)]
struct CtxId(usize);

/// 値をそのまま調べる文脈 (docs/spec/core-ir.md)。
enum Ctx {
    /// 条件の文脈 `[True] -> on_true | [False] -> on_false`。どちらのラベルも引数を持たない。値が分からなければ、
    /// `unknown` に値を渡し、そこで `switch` する。
    Bool {
        on_true: Label,
        on_false: Label,
        unknown: Label,
    },
    /// `match`、分解する `let`、分解する引数の文脈。枝のパターンと枝ごとのラベルを持つ (`pattern.rs`)。
    Match(MatchCtx),
}

/// 位置 (`Loc`) を作るための、本体のファイル。`Program.files` には、位置を作るときに初めて入れる
/// (docs/implementation/architecture.md)。
#[derive(Clone, Copy)]
struct Source<'a> {
    files: &'a SourceFiles,
    file_id: FileId,
}

/// トップレベルの本体1つを変換する間に変わらない参照。持ち上げた入れ子の関数も同じものを使う。
#[derive(Clone, Copy)]
struct BodyCtx<'a> {
    hir: &'a HirProgram,
    body: &'a Body,
    /// 型の表。型検査の表に、単相化の代入の結果を足したもの。本体の変換は読むだけである。
    store: &'a TypeStore,
    types: &'a BodyTypes,
    /// トップレベルの関数の instance の row のビット。本体の中の `RowYield::Var` を解く。
    bits: &'a [bool],
    /// 本体の中の、定義された関数とメソッドへの参照の行き先。
    targets: &'a ArenaMap<ExprId, Target>,
    /// instance の番号から関数の番号への表。
    indices: &'a [FnIdx],
    /// ラムダ、handle、extern を包む関数の名前に使う、トップレベルの関数の instance の名前。
    root_name: &'a str,
    /// 型ごとの包む関数の名前に使う表 (`instances::wrapper_name`)。
    names: &'a NameTable,
    numbering: &'a Numbering,
    /// 本体の中の節の `k` の変換の形。持ち上げた入れ子の関数も同じ表を見て、捕まえた `k` を同じ形で扱う。
    continuation_forms: &'a ArenaMap<LocalId, ContinuationForm>,
    /// 局所変数ごとの参照の数 (`references`)。
    references: &'a ArenaMap<LocalId, u32>,
    /// 本体のローカル関数ごとの持ち上げた関数 (`lifted_functions`)。
    lifted: &'a ArenaMap<LocalId, Lifted>,
    source: Source<'a>,
}

impl BodyCtx<'_> {
    /// 本体の中で記録した row の分類を、この instance の row のビットで解く。
    fn resolve(&self, row: RowYield) -> bool {
        instances::resolve(row, self.bits)
    }

    /// 定義された関数かメソッドへの参照 `expr` の行き先の関数。
    fn target(&self, expr: ExprId) -> FnIdx {
        match self.targets.get(expr) {
            Some(Target::Function(instance)) => self.indices[instance.0],
            Some(Target::Extern(_)) => {
                unreachable!("a method bound to an extern is called through `extern_target`")
            }
            None => unreachable!("every reference to a defined function or method has a target"),
        }
    }

    /// `extern` で結んだメソッドへの参照 `expr` なら、その行。
    fn extern_target(&self, expr: ExprId) -> Option<Extern> {
        match self.targets.get(expr) {
            Some(Target::Extern(row)) => Some(*row),
            Some(Target::Function(_)) | None => None,
        }
    }

    /// 作るクロージャの矢印 `closure_ty` が `Unr` なら、捕まえる値の型 `captured` がすべて `Unr` であることを
    /// 確かめる。参照の検査と操作の検査は両側を比べるので、両側が同じに誤った印 (`Lin` の値を持つ `unr` の
    /// クロージャ) には気づけないためである (docs/spec/core-ir.md の
    /// 「食い違いがないことの検査」)。
    fn check_captures(&self, closure_ty: TypeId, captured: &[TypeId], what: &str) {
        let store = self.store;
        let TypeKind::Fn { lin: Lin::Unr, .. } = store.kind(closure_ty) else {
            return;
        };
        for &ty in captured {
            let linearity = store.linearity(ty);
            assert!(
                linearity == Lin::Unr,
                "internal error: in `{}`, an unr {what} of type `{}` captures a value of type `{}`, whose linearity is {:?}",
                self.root_name,
                store.display_linear(closure_ty, &self.hir.names),
                store.display_linear(ty, &self.hir.names),
                linearity,
            );
        }
    }

    /// 型 `ty` の値の Repr と印。
    fn typed(&self, program: &mut ProgramBuilder, ty: TypeId) -> (Repr, Mark) {
        (
            program.repr(self.hir, self.store, ty),
            mark(self.store, ty, self.hir),
        )
    }

    /// 型が `lambda_ty` のラムダの引数と、本体の値の Repr と印。
    fn lambda_signature(
        &self,
        program: &mut ProgramBuilder,
        lambda_ty: TypeId,
        pats: &[PatId],
    ) -> (Vec<Param>, (Repr, Mark)) {
        let (param_types, ret_ty) = split_arrows(self.store, lambda_ty, pats.len());
        let params = pats
            .iter()
            .zip(&param_types)
            .map(|(&pat, &ty)| {
                let (repr, mark) = self.typed(program, ty);
                (Some(pat), repr, mark)
            })
            .collect();
        (params, self.typed(program, ret_ty))
    }

    /// 捕まえる変数と、その値の Repr と印。
    fn typed_captures(
        &self,
        program: &mut ProgramBuilder,
        captured: &[LocalId],
    ) -> Vec<(LocalId, Repr, Mark)> {
        captured
            .iter()
            .map(|&local| {
                let ty = *self.types.locals.get(local).expect("every local is typed");
                let (repr, mark) = self.typed(program, ty);
                (local, repr, mark)
            })
            .collect()
    }
}

struct FnLowering<'a> {
    ctx: BodyCtx<'a>,
    program: &'a mut ProgramBuilder,
    builder: FnBuilder,
    locals: ArenaMap<LocalId, Atom>,
    contexts: Vec<Ctx>,
    /// この関数で `con` で作った変数の中身。決定木が頭のコンストラクタを知るのに使う。
    cons: HashMap<VarId, Known>,
    /// 決定木が分解した値のフィールドの変数から、分解した値への表。決定木の葉の drop が、枝の束縛した変数をどの値から
    /// 取り出したかをたどるのに使う (docs/spec/core-ir.md の「決定木の葉の `drop`」)。
    parents: HashMap<VarId, VarId>,
    /// 決定木が今の道で `unpack` した構造体の変数と、そのフィールドの変数。葉の `drop` が構造体をフィールドごとに
    /// 扱うのに使う (`pattern.rs` の `drop_matched_away`)。
    unpacked: HashMap<VarId, Vec<VarId>>,
    /// 参照が1つでない局所変数に束縛した `con` の変数。決定木はこれを開かない
    /// (docs/spec/core-ir.md の「translate が決める印」)。
    shared: HashSet<VarId>,
    /// 作っている関数が yield しうるか (docs/spec/core-ir.md の
    /// 「`yields` の印」)。この関数の中の handle の節も同じ印にする。
    yields: bool,
}

impl<'a> FnLowering<'a> {
    fn new(ctx: BodyCtx<'a>, program: &'a mut ProgramBuilder, yields: bool) -> Self {
        FnLowering {
            ctx,
            program,
            yields,
            builder: FnBuilder::new(),
            locals: ArenaMap::default(),
            contexts: Vec::new(),
            cons: HashMap::new(),
            parents: HashMap::new(),
            unpacked: HashMap::new(),
            shared: HashSet::new(),
        }
    }

    /// ラムダと handle の本体と節は、捕まえた変数を先頭の引数に持つ (docs/spec/core-ir.md)。トップレベルの関数では
    /// `captured` は空である。捕まえた変数と引数は、その値の Repr と印と組にして渡す。`ret` は本体の値の Repr と印で、
    /// 関数の `ret` と `ret_mark` になる。`internal` は、値として生まれる関数 (ラムダ、handle の本体と節) なら真である。
    fn lower(
        mut self,
        name: &str,
        internal: bool,
        captured: &[(LocalId, Repr, Mark)],
        params: &[Param],
        root: ExprId,
        ret: (Repr, Mark),
    ) -> CoreFn {
        let body = self.ctx.body;
        for &(local, repr, mark) in captured {
            let var = self
                .builder
                .param(named(&body.locals[local].name, repr, mark));
            self.locals.insert(local, Atom::Var(var));
        }
        let mut wrapped = Vec::new();
        let mut destructured = Vec::new();
        for &(pat, repr, mark) in params {
            // 値を調べるか分解するパターンは名前のない引数で受け、本体の前で分解する
            let pattern = pat.filter(|&pat| destructures(body, self.ctx.hir, pat));
            let local = pat
                .filter(|_| pattern.is_none())
                .and_then(|pat| body.pat_bindings(pat).first().copied());
            let name = local.map_or("p", |local| body.locals[local].name.as_str());
            let var = self.builder.param(named(name, repr, mark));
            if let Some(local) = local {
                self.locals.insert(local, Atom::Var(var));
                if self.ctx.continuation_forms.get(local) == Some(&ContinuationForm::Wrapped) {
                    wrapped.push((local, var));
                }
            }
            if let Some(pattern) = pattern {
                destructured.push((pattern, var, repr, mark));
            }
        }
        // 引数の変数をすべて作ってから包み、分解する。関数の引数の番号を、ほかの変数より前にそろえるため
        for (local, var) in wrapped {
            let wrapper = self.continuation_wrapper(local);
            // 包んだ `k` は生の継続と同じ値なので、`k` の型の矢印の線形性で決める
            let k_mark = mark(self.ctx.store, self.ctx.types.locals[local], self.ctx.hir);
            let closure = self.closure(wrapper, vec![Atom::Var(var)], k_mark);
            self.locals.insert(local, closure);
        }
        for (pat, var, repr, mark) in destructured {
            self.destructure(pat, Scrutinee::Occ(Occ::Atom(Atom::Var(var)), repr, mark));
        }
        self.tail_expr(root, Exit::Return);
        let (ret, ret_mark) = ret;
        self.builder
            .finish(name.to_string(), internal, self.yields, ret, ret_mark)
    }

    /// 節の `k` (局所変数 `local`) を包む、`k` の型ごとの関数。名前の型引数は、再開の値と状態 (状態のある handler
    /// だけ) と結果の型である (docs/spec/lir.md の「関数の ABI」)。
    fn continuation_wrapper(&mut self, local: LocalId) -> FnIdx {
        let (hir, store) = (self.ctx.hir, self.ctx.store);
        let arity = self.ctx.body.continuations[local];
        let (params, result) = split_arrows(store, self.ctx.types.locals[local], arity);
        let base = if arity == 2 { "cont$state" } else { "cont$" };
        let mut args = params.clone();
        args.push(result);
        let name = wrapper_name(store, self.ctx.names, base, &args);
        self.program
            .continuation_wrapper(hir, store, name, &params, result)
    }

    /// `root` を、捕まえた変数を先頭の引数に持つ関数に持ち上げ、そのクロージャを作る (docs/spec/core-ir.md)。ラムダと、
    /// handle の本体と節に使う。`captured` は HIR の捕まえる変数で、ローカル関数はその持ち上げの捕まえる変数に置き換えて
    /// 受ける (docs/spec/core-ir.md の「ローカル関数の持ち上げ」)。関数の番号は持ち上げる前に取るので、入れ子の持ち上げは
    /// 外側より後ろの番号になる。
    /// `yields` は持ち上げた関数の印である。クロージャの印は、型のあるラムダなら `typed` で、型のない handle の本体と
    /// 節では捕まえた値の印の結びにする
    /// (docs/spec/core-ir.md の「translate が決める印」)。`role` は持ち上げた関数の EP の役割で、ラムダは `Typed` で
    /// ある (docs/spec/lir.md の「関数の ABI」)。
    #[expect(
        clippy::too_many_arguments,
        reason = "the parts of one lifted function"
    )]
    fn lift(
        &mut self,
        name: String,
        role: Role,
        yields: bool,
        captured: Vec<LocalId>,
        params: &[Param],
        root: ExprId,
        ret: (Repr, Mark),
        typed: Option<Mark>,
    ) -> Atom {
        let captured = self
            .ctx
            .typed_captures(self.program, &lifted_captures(self.ctx.lifted, captured));
        let function = self.program.reserve(captured.len() + params.len());
        let mut core = FnLowering::new(self.ctx, &mut *self.program, yields)
            .lower(&name, true, &captured, params, root, ret);
        core.role = role;
        self.program.finish(function, core);
        let closure_mark = typed.unwrap_or_else(|| {
            captured
                .iter()
                .fold(Mark::Unr, |joined, &(_, _, mark)| joined.join(mark))
        });
        let atoms = captured
            .iter()
            .map(|&(local, _, _)| self.locals[local])
            .collect();
        self.closure(function, atoms, closure_mark)
    }

    /// 関数と渡した引数の値。引数がなければ関数の値にし、クロージャを確保しない (docs/spec/core-ir.md)。関数の値は
    /// 型によらず `tobj` である (`types.rs` の `repr`)。`mark` はクロージャを作るときの印である。
    fn closure(&mut self, target: FnIdx, args: Vec<Atom>, mark: Mark) -> Atom {
        if args.is_empty() {
            return Atom::Fn(target);
        }
        self.bind("c", Repr::TObj, mark, Rhs::MakeClosure(target, args))
    }

    fn pat_type(&self, pat: PatId) -> TypeId {
        self.ctx
            .types
            .pats
            .get(pat)
            .copied()
            .expect("every pattern is typed")
    }

    /// 式の位置。呼ばれる側の範囲の先頭 (演算子のトークンか extern の名前) を渡す (docs/spec/core-ir.md)。
    fn loc(&mut self, expr: ExprId) -> Loc {
        let Source { files, file_id } = self.ctx.source;
        let start = self.ctx.body.exprs[expr].range.start();
        let position = files.line_col(file_id, start);
        Loc {
            file: self.program.files.intern(files.path(file_id)),
            line: position.line,
            column: position.column,
        }
    }

    /// 式の値を `exit` に渡す。`if`、`match` と、文の後に続く値は、同じ出口のまま中へ進む。条件の `if` が入れ子でも、
    /// 内側の枝は外側の文脈に直接値を渡すので、真偽値を作らずに分かれる。
    fn tail_expr(&mut self, id: ExprId, exit: Exit) {
        let body = self.ctx.body;
        match &body.exprs[id].kind {
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.branch(*condition, *then_branch, *else_branch, exit),
            ExprKind::Match {
                scrutinee, arms, ..
            } => self.lower_match(*scrutinee, None, arms, exit),
            ExprKind::Block { stmts, tail, .. } => match self.fused_match(stmts, *tail) {
                Some((local, init, arms)) => {
                    self.stmts(&stmts[..stmts.len() - 1]);
                    self.lower_match(init, Some(local), arms, exit);
                }
                None => {
                    self.stmts(stmts);
                    match tail {
                        Some(tail) => self.tail_expr(*tail, exit),
                        None => self.deliver(exit, Occ::Atom(Atom::Unit)),
                    }
                }
            },
            ExprKind::Annot { expr, .. } => self.tail_expr(*expr, exit),
            _ => {
                let value = self.occurrence(id);
                self.deliver(exit, value);
            }
        }
    }

    /// `let x = S` の直後の本体が `match x` である形なら、`x`、`S` と `match` の枝。`S` を `match` の文脈で変換し、
    /// `x` は値全体を使う枝にだけ渡す (docs/spec/core-ir.md)。
    fn fused_match(
        &self,
        stmts: &[HirStmt],
        tail: Option<ExprId>,
    ) -> Option<(LocalId, ExprId, &'a [MatchArm])> {
        let body = self.ctx.body;
        let ExprKind::Match {
            scrutinee, arms, ..
        } = &body.exprs[tail?].kind
        else {
            return None;
        };
        let ExprKind::Path(Res::Local(local)) = body.exprs[*scrutinee].kind else {
            return None;
        };
        let Some(HirStmt::Let { pat, init, .. }) = stmts.last() else {
            return None;
        };
        matches!(body.pats[*pat].kind, PatKind::Bind(bound) if bound == local).then_some((
            local,
            *init,
            arms.as_slice(),
        ))
    }

    /// 値を `exit` に渡す。`return` と続きのラベルには値を作って渡し、文脈には出現のまま渡す。
    fn deliver(&mut self, exit: Exit, value: Occ) {
        match exit {
            Exit::Return => {
                let value = self.materialize(value);
                self.builder.terminate(Term::Return(value));
            }
            Exit::Jump(label) => {
                let value = self.materialize(value);
                self.builder.jump(label, vec![value]);
            }
            Exit::Scrutinize(ctx) => self.select(ctx, value),
        }
    }

    /// 文脈に値を渡す。値が分かれば行き先へ直接向かい、分からなければ値の分からない出口として扱う。
    fn select(&mut self, ctx: CtxId, value: Occ) {
        match &self.contexts[ctx.0] {
            &Ctx::Bool {
                on_true,
                on_false,
                unknown,
            } => match value {
                Occ::Atom(Atom::Tag(TRUE)) => self.builder.jump(on_true, Vec::new()),
                Occ::Atom(Atom::Tag(FALSE)) => self.builder.jump(on_false, Vec::new()),
                value => {
                    let value = self.materialize(value);
                    self.builder.jump(unknown, vec![value]);
                }
            },
            Ctx::Match(_) => self.select_arm(ctx, value),
        }
    }

    /// `if c a b` を `match c with True -> a | False -> b` と同じに扱う (docs/spec/core-ir.md)。
    /// 条件は `Bool` の文脈で変換し、値が分からなかった位置にだけ `switch` を置く。枝は then、else の順に、そこへ
    /// 向かうブロックがあるときだけ変換する。
    fn branch(
        &mut self,
        condition: ExprId,
        then_branch: ExprId,
        else_branch: Option<ExprId>,
        exit: Exit,
    ) {
        let on_true = self.builder.new_label(Vec::new());
        let on_false = self.builder.new_label(Vec::new());
        let ty = self.ty(condition);
        let info = self.var_info("c", ty);
        let unknown = self.builder.new_label(vec![info]);
        self.contexts.push(Ctx::Bool {
            on_true,
            on_false,
            unknown,
        });
        let ctx = CtxId(self.contexts.len() - 1);
        self.tail_expr(condition, Exit::Scrutinize(ctx));
        if let Some(args) = self.builder.resolve(unknown) {
            let [scrutinee] = args[..] else {
                unreachable!("the unknown label takes the value");
            };
            self.switch_bool(scrutinee, on_true, on_false);
        }
        if self.builder.resolve(on_true).is_some() {
            self.tail_expr(then_branch, exit);
        }
        if self.builder.resolve(on_false).is_some() {
            match else_branch {
                Some(else_branch) => self.tail_expr(else_branch, exit),
                // `else` のない `if` の値は `()` である
                None => self.deliver(exit, Occ::Atom(Atom::Unit)),
            }
        }
    }

    /// 真偽値で分かれる `switch`。行き先は新しいブロックで、それぞれ枝のラベルへ向かう。枝のラベルに2本以上が
    /// 向かえば、行き先のブロックは `jump` だけを持つ辺のブロックになる (docs/spec/core-ir.md の R3)。
    fn switch_bool(&mut self, scrutinee: Atom, on_true: Label, on_false: Label) {
        let targets = [(FALSE, on_false), (TRUE, on_true)].map(|(tag, label)| {
            let block = self.builder.new_block();
            (tag, label, block)
        });
        let layout = self
            .program
            .data_layout(self.ctx.hir, self.ctx.store, self.ctx.hir.lang.bool);
        self.builder.terminate(Term::Switch {
            scrutinee,
            layout: Some(layout),
            cases: targets
                .iter()
                .map(|&(tag, _, target)| Case {
                    pattern: CasePattern::Tag(tag),
                    fields: Vec::new(),
                    target,
                })
                .collect(),
            default: None,
        });
        for (_, label, block) in targets {
            self.builder.reopen(block);
            self.builder.jump(label, Vec::new());
        }
    }

    fn stmts(&mut self, stmts: &[HirStmt]) {
        for stmt in stmts {
            match stmt {
                HirStmt::Let { pat, init, .. } => {
                    if destructures(self.ctx.body, self.ctx.hir, *pat) {
                        self.destructure(*pat, Scrutinee::Expr(*init));
                    } else {
                        let value = self.atom(*init);
                        self.bind_pat(*pat, value);
                    }
                }
                // 本体を変換する前に持ち上げてあるので、文の位置では何も出さない
                // (docs/spec/core-ir.md の「ローカル関数の持ち上げ」)
                HirStmt::Functions(_) => {}
                // 式文の値は `Unit` なので捨ててよい
                HirStmt::Expr(expr) => {
                    self.atom(*expr);
                }
            }
        }
    }

    /// `_` と `()` で受けた値は以後使われないので、Perceus が decref する。
    fn bind_pat(&mut self, pat: PatId, value: Atom) {
        for local in self.ctx.body.pat_bindings(pat) {
            self.bind_local(local, value);
        }
    }

    /// 局所変数を値に対応させる。参照が1つでない局所変数に `con` の変数を束縛すると、その変数は `shared` に入る。
    /// `con` の変数をほかの制御の道でも使うときに、決定木が開いて引数を読むと、Perceus が引数を `dup` するためである。
    fn bind_local(&mut self, local: LocalId, value: Atom) {
        if let Atom::Var(var) = value
            && self.ctx.references.get(local).copied().unwrap_or(0) != 1
        {
            self.shared.insert(var);
        }
        self.locals.insert(local, value);
    }
}
