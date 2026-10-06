//! 型付き HIR から Core IR への変換 (docs/spec/core-ir.md)。式の値の渡し先と join point の組み立てという、制御の
//! 骨組みをここに置く。式ごとの変換は `expr.rs`、関数の表と包む関数は `program.rs`、型から決まる変数の性質と
//! 組み込みの変換の種類は `types.rs` にある。

mod expr;
mod pattern;
mod program;
mod types;

use eml_hir::{Body, ExprId, ExprKind, FunctionId, LocalId, Module, PatId, Stmt};
use eml_types::{BodyTypes, Type, TypedModule};
use la_arena::ArenaMap;

use crate::builder::FnBuilder;
use crate::{Arm, Atom, CExpr, CExprId, CoreFn, FALSE, FnIdx, JoinId, Program, Rhs, TRUE, VarId};

use pattern::needs_decision_tree;
use program::{ProgramBuilder, effect_table};
use types::{split_arrows, var_info};

/// 誤りのない型付き HIR を、RC の命令のない Core IR にする。`captures` は空のままでよく、パイプラインが埋める
/// (docs/spec/core-ir.md のパスの表)。
pub(crate) fn translate(module: &Module, typed: &TypedModule) -> Program {
    let mut builder = ProgramBuilder::new(typed);
    let mut indices = ArenaMap::default();
    for (id, function) in module.functions.iter() {
        let body = function
            .body
            .as_ref()
            .expect("a program without errors has an equation for every function");
        indices.insert(id, builder.reserve(body.params.len()));
    }
    for (id, function) in module.functions.iter() {
        let body = function.body.as_ref().expect("checked above");
        let signature = &typed
            .signatures
            .get(id)
            .expect("every function has a signature")
            .ty;
        let (param_types, _) = split_arrows(signature, body.params.len());
        let params: Vec<(Option<PatId>, Type)> = body
            .params
            .iter()
            .map(|&pat| Some(pat))
            .zip(param_types)
            .collect();
        let mut lambdas = 0;
        let mut handlers = 0;
        let core = FnLowering {
            module,
            body,
            types: typed.bodies.get(id).expect("every body is type-checked"),
            indices: &indices,
            program: &mut builder,
            root_name: &function.name,
            lambdas: &mut lambdas,
            handlers: &mut handlers,
            builder: FnBuilder::new(),
            locals: ArenaMap::default(),
        }
        .lower(&function.name, &[], &params, body.root);
        builder.finish(indices[id], core);
    }
    let main = typed
        .main
        .expect("`eml_cli::compile` reports a missing `main`");
    let main_type = &typed
        .signatures
        .get(main)
        .expect("`main` has a signature")
        .ty;
    let entry = builder.entry(module, indices[main], main_type);
    Program {
        functions: builder
            .functions
            .into_iter()
            .map(|function| function.expect("every reserved function is lowered"))
            .collect(),
        entry,
        strings: builder.strings.values,
        effects: effect_table(module),
    }
}

/// 値を計算する束縛と、join point の開始の並び。`seq` が後ろから組み立てる。
enum Binding {
    Let(VarId, Rhs),
    /// ここより後ろで組み立てる式を本体にし、`scope` (条件の計算と、枝が `Jump` する `Switch`) を範囲にする join point。
    Join {
        join: JoinId,
        params: Vec<VarId>,
        scope: CExprId,
    },
    /// 本体を先に組み立てた join point。ここより後ろで組み立てる式を範囲にする。`match` の枝と、決定木の残りの
    /// 部分木に使う (`pattern.rs`)。
    Shared {
        join: JoinId,
        params: Vec<VarId>,
        body: CExprId,
    },
}

type Bindings = Vec<Binding>;

/// 式の値の渡し先。
#[derive(Clone, Copy)]
enum Exit {
    Return,
    Jump(JoinId),
}

fn exit_with(exit: Exit, value: Atom) -> CExpr {
    match exit {
        Exit::Return => CExpr::Return(value),
        Exit::Jump(join) => CExpr::Jump {
            join,
            args: vec![value],
        },
    }
}

struct FnLowering<'a> {
    module: &'a Module,
    body: &'a Body,
    types: &'a BodyTypes,
    indices: &'a ArenaMap<FunctionId, FnIdx>,
    program: &'a mut ProgramBuilder,
    /// ラムダの関数の名前に使う、トップレベルの関数の名前と、その中のラムダの数。
    root_name: &'a str,
    lambdas: &'a mut u32,
    /// handle の本体と節の関数の名前に使う、トップレベルの関数の中の handle の数。
    handlers: &'a mut u32,
    builder: FnBuilder,
    locals: ArenaMap<LocalId, Atom>,
}

impl FnLowering<'_> {
    /// ラムダと handle の本体と節は、捕まえた変数を先頭の引数に持つ (docs/spec/core-ir.md)。トップレベルの関数では
    /// `captured` は空である。引数のパターンが `None` なら、名前のない引数 (handle の本体が受ける `()`) である。
    fn lower(
        mut self,
        name: &str,
        captured: &[(LocalId, Type)],
        params: &[(Option<PatId>, Type)],
        root: ExprId,
    ) -> CoreFn {
        let body = self.body;
        let mut vars = Vec::new();
        for (local, ty) in captured {
            let var = self.new_var(&body.locals[*local].name, ty);
            self.locals.insert(*local, Atom::Var(var));
            vars.push(var);
        }
        let mut destructured = Vec::new();
        for (pat, ty) in params {
            // 値を調べるか分解するパターンは名前のない引数で受け、本体の前で分解する
            let simple = pat.filter(|&pat| !needs_decision_tree(body, pat));
            let local = simple.and_then(|pat| body.pat_bindings(pat).first().copied());
            let name = local.map_or("p", |local| body.locals[local].name.as_str());
            let var = self.new_var(name, ty);
            if let Some(local) = local {
                self.locals.insert(local, Atom::Var(var));
            }
            vars.push(var);
            if let Some(pat) = pat.filter(|&pat| needs_decision_tree(body, pat)) {
                destructured.push((pat, var, ty.clone()));
            }
        }
        // 引数の変数をすべて作ってから分解する。関数の引数の番号を、分解で作る変数より前にそろえるため
        let mut bindings = Vec::new();
        for (pat, var, ty) in destructured {
            self.destructure(pat, Atom::Var(var), ty, &mut bindings);
        }
        let root = self.tail_after(bindings, root, Exit::Return);
        self.builder.finish(name.to_string(), vars, root)
    }

    /// `root` を、捕まえた変数を先頭の引数に持つ関数に持ち上げ、そのクロージャを作る (docs/spec/core-ir.md)。ラムダと、
    /// handle の本体と節に使う。関数の番号は持ち上げる前に取るので、入れ子の持ち上げは外側より後ろの番号になる。
    fn lift(
        &mut self,
        name: String,
        captured: Vec<LocalId>,
        params: &[(Option<PatId>, Type)],
        root: ExprId,
        ty: &Type,
        out: &mut Bindings,
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
            module: self.module,
            body: self.body,
            types: self.types,
            indices: self.indices,
            program: &mut *self.program,
            root_name: self.root_name,
            lambdas: &mut *self.lambdas,
            handlers: &mut *self.handlers,
            builder: FnBuilder::new(),
            locals: ArenaMap::default(),
        }
        .lower(&name, &captured, params, root);
        self.program.finish(function, core);
        let atoms = captured
            .iter()
            .map(|(local, _)| self.locals[*local])
            .collect();
        self.bind(out, "c", ty, Rhs::MakeClosure(function, atoms))
    }

    fn pat_type(&self, pat: PatId) -> Type {
        self.types
            .pats
            .get(pat)
            .cloned()
            .expect("every pattern is typed")
    }

    fn new_var(&mut self, name: &str, ty: &Type) -> VarId {
        self.builder.var(var_info(name, ty, self.module))
    }

    fn push(&mut self, expr: CExpr) -> CExprId {
        self.builder.push(expr)
    }

    fn new_join(&mut self) -> JoinId {
        self.builder.new_join()
    }

    fn seq(&mut self, bindings: Bindings, last: CExpr) -> CExprId {
        let mut id = self.push(last);
        for binding in bindings.into_iter().rev() {
            id = match binding {
                Binding::Let(var, rhs) => self.push(CExpr::Let { var, rhs, body: id }),
                Binding::Join {
                    join,
                    params,
                    scope,
                } => {
                    let expr = self.push(CExpr::Join {
                        join,
                        params,
                        captures: Vec::new(),
                        body: id,
                        scope,
                    });
                    self.builder.define_join(join, expr);
                    expr
                }
                Binding::Shared { join, params, body } => {
                    let expr = self.push(CExpr::Join {
                        join,
                        params,
                        captures: Vec::new(),
                        body,
                        scope: id,
                    });
                    self.builder.define_join(join, expr);
                    expr
                }
            };
        }
        id
    }

    /// 式の値を `exit` に渡すコード。末尾呼び出しは simplify の T が作る (docs/spec/core-ir.md)。
    fn tail(&mut self, expr: ExprId, exit: Exit) -> CExprId {
        self.tail_after(Vec::new(), expr, exit)
    }

    /// `bindings` (引数のパターンの分解) の後に、式の値を `exit` に渡すコードを続ける。
    fn tail_after(&mut self, mut bindings: Bindings, expr: ExprId, exit: Exit) -> CExprId {
        let last = self.tail_expr(expr, exit, &mut bindings);
        self.seq(bindings, last)
    }

    /// 式の値を `exit` に渡す最後の命令を返す。値の計算に要る束縛は `out` に積む。末尾の `if` は、枝が直接 `exit` に
    /// 渡す `Switch` にし、join point を作らない。
    fn tail_expr(&mut self, id: ExprId, exit: Exit, out: &mut Bindings) -> CExpr {
        let body = self.body;
        match &body.exprs[id].kind {
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let scrutinee = self.atom(*condition, out);
                let then_code = self.tail(*then_branch, exit);
                let else_code = match else_branch {
                    Some(else_branch) => self.tail(*else_branch, exit),
                    // `else` のない `if` の値は `()` である
                    None => self.push(exit_with(exit, Atom::Unit)),
                };
                CExpr::Switch {
                    scrutinee,
                    arms: vec![
                        Arm {
                            tag: FALSE,
                            fields: Vec::new(),
                            body: else_code,
                        },
                        Arm {
                            tag: TRUE,
                            fields: Vec::new(),
                            body: then_code,
                        },
                    ],
                }
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => self.lower_match(*scrutinee, arms, exit, out),
            ExprKind::Block { stmts, tail, .. } => {
                self.stmts(stmts, out);
                match tail {
                    Some(tail) => self.tail_expr(*tail, exit, out),
                    None => exit_with(exit, Atom::Unit),
                }
            }
            ExprKind::Annot { expr, .. } => self.tail_expr(*expr, exit, out),
            _ => {
                let value = self.atom(id, out);
                exit_with(exit, value)
            }
        }
    }

    fn stmts(&mut self, stmts: &[Stmt], out: &mut Bindings) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { pat, init, .. } => {
                    let value = self.atom(*init, out);
                    if needs_decision_tree(self.body, *pat) {
                        let ty = self.ty(*init);
                        self.destructure(*pat, value, ty, out);
                    } else {
                        self.bind_pat(*pat, value);
                    }
                }
                // 式文の値は `Unit` なので捨ててよい
                Stmt::Expr(expr) => {
                    self.atom(*expr, out);
                }
            }
        }
    }
}
