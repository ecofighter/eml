//! 型付き HIR から、前向きの辺だけを持つブロックの列への変換 (docs/spec/core-ir.md)。式の値の渡し先 (出口) と、
//! 条件の分かれ方という制御の骨組みをここに置く。ブロックの組み立ては `builder.rs`、式ごとの変換は `expr.rs`、
//! パターンの決定木と case-of-case は `pattern.rs`、関数の表と包む関数は `program.rs`、型から決まる Repr は
//! `types.rs` にある。

mod builder;
mod expr;
mod instances;
mod pattern;
mod program;
mod types;

use std::collections::{HashMap, HashSet};

use eml_diagnostics::{FileId, SourceFiles};
use eml_hir::{
    Body, ExprId, ExprKind, FunctionId, LocalId, MatchArm, PatId, PatKind, Program as HirProgram,
    Res, Stmt as HirStmt, ValueItem,
};
use eml_types::{BodyTypes, TypeId, TypeStore, TypedProgram};
use la_arena::ArenaMap;

use crate::{
    Atom, Case, CasePattern, CoreFn, FALSE, FnIdx, Loc, Program, Repr, Rhs, TRUE, Term, VarId,
};

use builder::{FnBuilder, Label};
use expr::extern_row;
use instances::InstanceId;
use pattern::{Known, MatchCtx, Occ, Scrutinee, destructures};
use program::{ProgramBuilder, effect_table};
use types::{named, repr, split_arrows, var_info};

// extern の表の行と std の宣言を照らし合わせる結合テスト (tests/externs.rs) が、translate と同じ規則で型の Repr を決める
pub use types::repr as type_repr;

/// 節の `k` の変換の形 (docs/spec/core-ir.md)。
#[derive(Clone, Copy, PartialEq, Eq)]
enum ContinuationForm {
    /// `k` を生の継続のまま持ち、呼び出しを `Call::Resume` にする。`k` を捕まえた入れ子の関数でも生の継続のままである。
    Direct,
    /// 節の入口で、生の継続を `cont$` か `cont$state` のクロージャに包み、それを `k` とする。
    Wrapped,
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

/// 本体の中で関数を作る式の番号。`$lambdaN`、`$handleN`、`$externN` の N である。変換の順に依らず、同じ本体なら
/// 同じ名前になるように、本体を変換する前に式の ID の順で振る (docs/spec/core-ir.md)。
struct Numbering {
    lambdas: ArenaMap<ExprId, u32>,
    handlers: ArenaMap<ExprId, u32>,
    /// 値として使う extern の参照 (引数をそろえて呼ぶ位置にない参照と、部分適用の呼ばれる式)。
    externs: ArenaMap<ExprId, u32>,
}

fn numbering(hir: &HirProgram, body: &Body) -> Numbering {
    // 引数をそろえて呼ぶ extern の参照は `Rhs::Extern` になり、包む関数を作らない
    let saturated: HashSet<ExprId> = body
        .exprs
        .iter()
        .filter_map(|(_, expr)| match &expr.kind {
            ExprKind::Call { callee, args } => {
                let ExprKind::Path(Res::Item(ValueItem::Function(function))) =
                    body.exprs[*callee].kind
                else {
                    return None;
                };
                let row = extern_row(hir, function)?;
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
            ExprKind::Lambda(_) => lambdas.push(id),
            ExprKind::Handle { .. } => handlers.push(id),
            ExprKind::Path(Res::Item(ValueItem::Function(function)))
                if extern_row(hir, *function).is_some() && !saturated.contains(&id) =>
            {
                externs.push(id)
            }
            _ => {}
        }
    }
    let number = |ids: Vec<ExprId>| ids.into_iter().zip(0..).collect();
    Numbering {
        lambdas: number(lambdas),
        handlers: number(handlers),
        externs: number(externs),
    }
}

/// 誤りのない型付き HIR を、RC の命令と末尾呼び出しのない Core IR にする。末尾呼び出しは縮約が作る。
pub(crate) fn translate(
    hir: &HirProgram,
    typed: &TypedProgram,
    entry: FunctionId,
    files: &SourceFiles,
) -> Program {
    let mut builder = ProgramBuilder::new(hir, typed);
    let instances = instances::collect(hir, typed, entry);
    let store = &instances.store;
    let indices: Vec<FnIdx> = instances
        .list
        .iter()
        .map(|instance| {
            let body = hir
                .body(instance.function)
                .expect("a program without errors has an equation for every function");
            builder.reserve(body.params.len())
        })
        .collect();
    for (instance, &index) in instances.list.iter().zip(&indices) {
        let id = instance.function;
        let body = hir.body(id).expect("checked above");
        let (param_types, ret) = split_arrows(store, instance.signature, body.params.len());
        let params: Vec<(Option<PatId>, Repr)> = body
            .params
            .iter()
            .zip(&param_types)
            .map(|(&pat, &ty)| (Some(pat), repr(store, ty, hir)))
            .collect();
        let forms = continuation_forms(body);
        let numbers = numbering(hir, body);
        let ctx = BodyCtx {
            hir,
            body,
            store,
            types: instance
                .types
                .as_ref()
                .unwrap_or_else(|| typed.bodies.get(id).expect("every body is type-checked")),
            targets: &instance.targets,
            indices: &indices,
            root_name: &instance.name,
            numbering: &numbers,
            continuation_forms: &forms,
            source: Source {
                files,
                file_id: hir.modules[id.module].file,
            },
        };
        let core = FnLowering::new(ctx, &mut builder).lower(
            &instance.name,
            false,
            &[],
            &params,
            body.root,
            repr(store, ret, hir),
        );
        builder.finish(index, core);
    }
    let entry_instance = &instances.list[instances.entry.0];
    let entry_fn = builder.entry(
        hir,
        store,
        indices[instances.entry.0],
        entry,
        entry_instance.signature,
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
        effects: effect_table(hir),
        files: builder.files.values,
    }
}

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
    /// 本体の中の、定義された関数への参照の行き先の instance。
    targets: &'a ArenaMap<ExprId, InstanceId>,
    /// instance の番号から関数の番号への表。
    indices: &'a [FnIdx],
    /// ラムダ、handle、extern を包む関数の名前に使う、トップレベルの関数の名前。
    root_name: &'a str,
    numbering: &'a Numbering,
    /// 本体の中の節の `k` の変換の形。持ち上げた入れ子の関数も同じ表を見て、捕まえた `k` を同じ形で扱う。
    continuation_forms: &'a ArenaMap<LocalId, ContinuationForm>,
    source: Source<'a>,
}

impl BodyCtx<'_> {
    /// 定義された関数への参照 `expr` の行き先の関数。
    fn target(&self, expr: ExprId) -> FnIdx {
        let instance = self
            .targets
            .get(expr)
            .expect("every reference to a defined function has an instance");
        self.indices[instance.0]
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
}

impl<'a> FnLowering<'a> {
    fn new(ctx: BodyCtx<'a>, program: &'a mut ProgramBuilder) -> Self {
        FnLowering {
            ctx,
            program,
            builder: FnBuilder::new(),
            locals: ArenaMap::default(),
            contexts: Vec::new(),
            cons: HashMap::new(),
        }
    }

    /// ラムダと handle の本体と節は、捕まえた変数を先頭の引数に持つ (docs/spec/core-ir.md)。トップレベルの関数では
    /// `captured` は空である。引数のパターンが `None` なら、名前のない引数 (handle の本体が受ける `()`) である。
    /// 捕まえた変数と引数は、その値の Repr と組にして渡す。`ret` は本体の値の Repr で、関数の `ret` になる。
    /// `internal` は、持ち上げた関数なら真である。
    fn lower(
        mut self,
        name: &str,
        internal: bool,
        captured: &[(LocalId, Repr)],
        params: &[(Option<PatId>, Repr)],
        root: ExprId,
        ret: Repr,
    ) -> CoreFn {
        let body = self.ctx.body;
        for &(local, repr) in captured {
            let var = self.builder.param(named(&body.locals[local].name, repr));
            self.locals.insert(local, Atom::Var(var));
        }
        let mut wrapped = Vec::new();
        let mut destructured = Vec::new();
        for &(pat, repr) in params {
            // 値を調べるか分解するパターンは名前のない引数で受け、本体の前で分解する
            let pattern = pat.filter(|&pat| destructures(body, self.ctx.hir, pat));
            let local = pat
                .filter(|_| pattern.is_none())
                .and_then(|pat| body.pat_bindings(pat).first().copied());
            let name = local.map_or("p", |local| body.locals[local].name.as_str());
            let var = self.builder.param(named(name, repr));
            if let Some(local) = local {
                self.locals.insert(local, Atom::Var(var));
                if self.ctx.continuation_forms.get(local) == Some(&ContinuationForm::Wrapped) {
                    wrapped.push((local, var));
                }
            }
            if let Some(pattern) = pattern {
                destructured.push((pattern, var, repr));
            }
        }
        // 引数の変数をすべて作ってから包み、分解する。関数の引数の番号を、ほかの変数より前にそろえるため
        for (local, var) in wrapped {
            let wrapper = self
                .program
                .continuation_wrapper(body.continuations[local] == 2);
            let closure = self.closure(wrapper, vec![Atom::Var(var)]);
            self.locals.insert(local, closure);
        }
        for (pat, var, repr) in destructured {
            self.destructure(pat, Scrutinee::Occ(Occ::Atom(Atom::Var(var)), repr));
        }
        self.tail_expr(root, Exit::Return);
        self.builder.finish(name.to_string(), internal, ret)
    }

    /// `root` を、捕まえた変数を先頭の引数に持つ関数に持ち上げ、そのクロージャを作る (docs/spec/core-ir.md)。ラムダと、
    /// handle の本体と節に使う。関数の番号は持ち上げる前に取るので、入れ子の持ち上げは外側より後ろの番号になる。
    fn lift(
        &mut self,
        name: String,
        captured: Vec<LocalId>,
        params: &[(Option<PatId>, Repr)],
        root: ExprId,
        ret: Repr,
    ) -> Atom {
        let captured: Vec<(LocalId, Repr)> = captured
            .into_iter()
            .map(|local| {
                let ty = self
                    .ctx
                    .types
                    .locals
                    .get(local)
                    .expect("every local is typed");
                (local, repr(self.ctx.store, *ty, self.ctx.hir))
            })
            .collect();
        let function = self.program.reserve(captured.len() + params.len());
        let core = FnLowering::new(self.ctx, &mut *self.program)
            .lower(&name, true, &captured, params, root, ret);
        self.program.finish(function, core);
        let atoms = captured
            .iter()
            .map(|(local, _)| self.locals[*local])
            .collect();
        self.closure(function, atoms)
    }

    /// 関数と渡した引数の値。引数がなければ関数の値にし、クロージャを確保しない (docs/spec/core-ir.md)。関数の値は
    /// 型によらず `tobj` である (`types.rs` の `repr`)。
    fn closure(&mut self, target: FnIdx, args: Vec<Atom>) -> Atom {
        if args.is_empty() {
            return Atom::Fn(target);
        }
        self.bind("c", Repr::TObj, Rhs::MakeClosure(target, args))
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
        let unknown = self
            .builder
            .new_label(vec![var_info("c", self.ctx.store, ty, self.ctx.hir)]);
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
            self.locals.insert(local, value);
        }
    }
}
