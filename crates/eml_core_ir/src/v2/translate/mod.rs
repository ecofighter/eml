//! 型付き HIR から、前向きの辺だけを持つブロックの列への変換 (docs/spec/core-ir.md)。式の値の渡し先 (出口) と、
//! 条件の分かれ方という制御の骨組みをここに置く。ブロックの組み立ては `builder.rs`、式ごとの変換は `expr.rs`、関数の
//! 表と包む関数は `program.rs`、型から決まる Repr は `types.rs` にある。

mod builder;
mod expr;
mod program;
mod types;

use std::collections::HashSet;

use eml_diagnostics::{FileId, SourceFiles};
use eml_hir::{
    Body, ExprId, ExprKind, Function, FunctionId, FunctionKind, ItemMap, LocalId, PatId, PatKind,
    Program as HirProgram, Res, Stmt as HirStmt, ValueItem,
};
use eml_types::{BodyTypes, Type, TypedProgram};
use la_arena::ArenaMap;

use crate::v2::{Case, CoreFn, Loc, Program, Rhs, Term};
use crate::{Atom, CasePattern, FALSE, FnIdx, TRUE};

use builder::{FnBuilder, Label};
use expr::extern_row;
use program::{ProgramBuilder, core_name, effect_table};
use types::{repr, split_arrows, var_info};

/// 入口の関数から届く関数。使わない Prelude の関数を Core IR に入れないため、関数の本体の参照をたどって集める
/// (docs/spec/core-ir.md)。
fn reachable(hir: &HirProgram, entry: FunctionId) -> HashSet<FunctionId> {
    let mut seen = HashSet::new();
    let mut work = vec![entry];
    while let Some(id) = work.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(body) = hir.body(id) else {
            continue;
        };
        for (_, expr) in body.exprs.iter() {
            if let ExprKind::Path(Res::Item(ValueItem::Function(callee))) = expr.kind {
                work.push(callee);
            }
        }
    }
    seen
}

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
                (args.len() >= row.row().arity).then_some(*callee)
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

/// 誤りのない型付き HIR を、RC の命令のない Core IR にする。末尾呼び出しは各関数の `finish` が作る。
pub(crate) fn translate(
    hir: &HirProgram,
    typed: &TypedProgram,
    entry: FunctionId,
    files: &SourceFiles,
) -> Program {
    let mut builder = ProgramBuilder::new(hir, typed);
    let mut indices = ItemMap::default();
    let reached = reachable(hir, entry);
    // extern の関数は本体を持たず、呼び出しの位置で `Rhs::Extern` にするか、包む関数を作る (`program.rs` の `extern_wrapper`)
    let defined = || {
        hir.functions()
            .filter(|(id, function)| function.kind == FunctionKind::Defined && reached.contains(id))
    };
    for (id, _) in defined() {
        let body = hir
            .body(id)
            .expect("a program without errors has an equation for every function");
        indices.insert(id, builder.reserve(body.params.len()));
    }
    for (id, function) in defined() {
        let body = hir.body(id).expect("checked above");
        let signature = &typed
            .decls
            .get(&ValueItem::Function(id))
            .expect("every function has a signature")
            .ty;
        let (param_types, ret) = split_arrows(signature, body.params.len());
        let params: Vec<(Option<PatId>, Type)> = body
            .params
            .iter()
            .map(|&pat| Some(pat))
            .zip(param_types)
            .collect();
        let name = core_name(hir, id.module, &function.name);
        let forms = continuation_forms(body);
        let numbers = numbering(hir, body);
        let file_id = hir.modules[id.module].file;
        let file = builder.files.intern(files.path(file_id));
        let core = FnLowering {
            hir,
            body,
            types: typed.bodies.get(id).expect("every body is type-checked"),
            indices: &indices,
            program: &mut builder,
            root_name: &name,
            numbering: &numbers,
            continuation_forms: &forms,
            source: Source {
                files,
                file_id,
                file,
            },
            builder: FnBuilder::new(),
            locals: ArenaMap::default(),
            contexts: Vec::new(),
        }
        .lower(&name, &[], &params, body.root, &ret);
        builder.finish(indices[id], core);
    }
    let entry_type = &typed
        .decls
        .get(&ValueItem::Function(entry))
        .expect("the entry function has a signature")
        .ty;
    let entry_fn = builder.entry(hir, indices[entry], entry, entry_type);
    Program {
        functions: builder
            .functions
            .into_iter()
            .map(|function| function.expect("every reserved function is lowered"))
            .collect(),
        entry: entry_fn,
        strings: builder.strings.values,
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
}

/// 位置 (`Loc`) を作るための、本体のファイル。
#[derive(Clone, Copy)]
struct Source<'a> {
    files: &'a SourceFiles,
    file_id: FileId,
    /// `Program.files` の添字。
    file: u32,
}

struct FnLowering<'a> {
    hir: &'a HirProgram,
    body: &'a Body,
    types: &'a BodyTypes,
    indices: &'a ItemMap<Function, FnIdx>,
    program: &'a mut ProgramBuilder,
    /// ラムダ、handle、extern を包む関数の名前に使う、トップレベルの関数の名前。
    root_name: &'a str,
    numbering: &'a Numbering,
    /// 本体の中の節の `k` の変換の形。持ち上げた入れ子の関数も同じ表を見て、捕まえた `k` を同じ形で扱う。
    continuation_forms: &'a ArenaMap<LocalId, ContinuationForm>,
    source: Source<'a>,
    builder: FnBuilder,
    locals: ArenaMap<LocalId, Atom>,
    contexts: Vec<Ctx>,
}

/// パターンが値を調べるか分解するか。どちらもしなければ、値をそのまま局所変数に対応させればよく、決定木は要らない。
fn destructures(body: &Body, pat: PatId) -> bool {
    match &body.pats[pat].kind {
        PatKind::Con { .. } | PatKind::Tuple(_) | PatKind::Literal(_) => true,
        PatKind::Annot { pat, .. } => destructures(body, *pat),
        PatKind::Bind(_) | PatKind::Wildcard | PatKind::Unit | PatKind::Missing => false,
    }
}

impl FnLowering<'_> {
    /// ラムダと handle の本体と節は、捕まえた変数を先頭の引数に持つ (docs/spec/core-ir.md)。トップレベルの関数では
    /// `captured` は空である。引数のパターンが `None` なら、名前のない引数 (handle の本体が受ける `()`) である。
    /// `ret` は本体の値の型で、関数の `ret` の Repr を決める。
    fn lower(
        mut self,
        name: &str,
        captured: &[(LocalId, Type)],
        params: &[(Option<PatId>, Type)],
        root: ExprId,
        ret: &Type,
    ) -> CoreFn {
        let body = self.body;
        for (local, ty) in captured {
            let var = self
                .builder
                .param(var_info(&body.locals[*local].name, ty, self.hir));
            self.locals.insert(*local, Atom::Var(var));
        }
        let mut wrapped = Vec::new();
        for (pat, ty) in params {
            if pat.is_some_and(|pat| destructures(body, pat)) {
                unimplemented!("parameter patterns that take values apart");
            }
            let local = pat.and_then(|pat| body.pat_bindings(pat).first().copied());
            let name = local.map_or("p", |local| body.locals[local].name.as_str());
            let var = self.builder.param(var_info(name, ty, self.hir));
            if let Some(local) = local {
                self.locals.insert(local, Atom::Var(var));
                if self.continuation_forms.get(local) == Some(&ContinuationForm::Wrapped) {
                    wrapped.push((local, var, ty.clone()));
                }
            }
        }
        // 引数の変数をすべて作ってから包む。関数の引数の番号を、ほかの変数より前にそろえるため
        for (local, var, ty) in wrapped {
            let wrapper = self
                .program
                .continuation_wrapper(body.continuations[local] == 2);
            let closure = self.closure(wrapper, vec![Atom::Var(var)], &ty);
            self.locals.insert(local, closure);
        }
        self.tail_expr(root, Exit::Return);
        self.builder.finish(name.to_string(), repr(ret, self.hir))
    }

    /// `root` を、捕まえた変数を先頭の引数に持つ関数に持ち上げ、そのクロージャを作る (docs/spec/core-ir.md)。ラムダと、
    /// handle の本体と節に使う。関数の番号は持ち上げる前に取るので、入れ子の持ち上げは外側より後ろの番号になる。
    fn lift(
        &mut self,
        name: String,
        captured: Vec<LocalId>,
        params: &[(Option<PatId>, Type)],
        root: ExprId,
        ret: &Type,
        ty: &Type,
    ) -> Atom {
        let captured: Vec<(LocalId, Type)> = captured
            .into_iter()
            .map(|local| {
                let ty = self
                    .types
                    .locals
                    .get(local)
                    .cloned()
                    .expect("every local is typed");
                (local, ty)
            })
            .collect();
        let function = self.program.reserve(captured.len() + params.len());
        let core = FnLowering {
            hir: self.hir,
            body: self.body,
            types: self.types,
            indices: self.indices,
            program: &mut *self.program,
            root_name: self.root_name,
            numbering: self.numbering,
            continuation_forms: self.continuation_forms,
            source: self.source,
            builder: FnBuilder::new(),
            locals: ArenaMap::default(),
            contexts: Vec::new(),
        }
        .lower(&name, &captured, params, root, ret);
        self.program.finish(function, core);
        let atoms = captured
            .iter()
            .map(|(local, _)| self.locals[*local])
            .collect();
        self.closure(function, atoms, ty)
    }

    /// 関数と渡した引数の値。引数がなければ関数の値にし、クロージャを確保しない (docs/spec/core-ir.md)。
    fn closure(&mut self, target: FnIdx, args: Vec<Atom>, ty: &Type) -> Atom {
        if args.is_empty() {
            return Atom::Fn(target);
        }
        self.bind("c", ty, Rhs::MakeClosure(target, args))
    }

    fn pat_type(&self, pat: PatId) -> Type {
        self.types
            .pats
            .get(pat)
            .cloned()
            .expect("every pattern is typed")
    }

    /// 式の位置。呼ばれる側の範囲の先頭 (演算子のトークンか extern の名前) を渡す (docs/spec/core-ir.md)。
    fn loc(&self, expr: ExprId) -> Loc {
        let start = self.body.exprs[expr].range.start();
        let position = self.source.files.line_col(self.source.file_id, start);
        Loc {
            file: self.source.file,
            line: position.line,
            column: position.column,
        }
    }

    /// 式の値を `exit` に渡す。`if` と、文の後に続く値は、同じ出口のまま中へ進む。条件の `if` が入れ子でも、内側の
    /// 枝は外側の文脈に直接値を渡すので、真偽値を作らずに分かれる。
    fn tail_expr(&mut self, id: ExprId, exit: Exit) {
        let body = self.body;
        match &body.exprs[id].kind {
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.branch(*condition, *then_branch, *else_branch, exit),
            ExprKind::Match { .. } => unimplemented!("`match` in the block-list translate"),
            ExprKind::Block { stmts, tail, .. } => {
                self.stmts(stmts);
                match tail {
                    Some(tail) => self.tail_expr(*tail, exit),
                    None => self.deliver(exit, Atom::Unit),
                }
            }
            ExprKind::Annot { expr, .. } => self.tail_expr(*expr, exit),
            _ => {
                let value = self.atom(id);
                self.deliver(exit, value);
            }
        }
    }

    fn deliver(&mut self, exit: Exit, value: Atom) {
        match exit {
            Exit::Return => self.builder.terminate(Term::Return(value)),
            Exit::Jump(label) => self.builder.jump(label, vec![value]),
            Exit::Scrutinize(ctx) => self.select(ctx, value),
        }
    }

    /// 文脈に値を渡す。値が分かれば行き先へ直接向かい、分からなければ `unknown` へ向かう。
    fn select(&mut self, ctx: CtxId, value: Atom) {
        match self.contexts[ctx.0] {
            Ctx::Bool {
                on_true,
                on_false,
                unknown,
            } => match value {
                Atom::Tag(TRUE) => self.builder.jump(on_true, Vec::new()),
                Atom::Tag(FALSE) => self.builder.jump(on_false, Vec::new()),
                _ => self.builder.jump(unknown, vec![value]),
            },
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
        let unknown = self.builder.new_label(vec![var_info("c", &ty, self.hir)]);
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
                None => self.deliver(exit, Atom::Unit),
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
        self.builder.terminate(Term::Switch {
            scrutinee,
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
                    if destructures(self.body, *pat) {
                        unimplemented!("`let` patterns that take values apart");
                    }
                    let value = self.atom(*init);
                    self.bind_pat(*pat, value);
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
        for local in self.body.pat_bindings(pat) {
            self.locals.insert(local, value);
        }
    }
}
