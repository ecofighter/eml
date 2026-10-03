use std::collections::{BTreeSet, HashMap, HashSet};

use eml_hir::builtin::Builtin;
use eml_hir::{
    Body, ExprId, ExprKind, FunctionId, Literal, LocalId, Module, PatId, PatKind, Res, Stmt,
};
use eml_types::{BodyTypes, Linearity, Type, TypedModule};
use la_arena::ArenaMap;

use crate::{
    Atom, CExpr, CExprId, CoreFn, FALSE, FnIdx, IoOp, PrimOp, Program, Rhs, TRUE, VarId, VarInfo,
    perceus,
};

/// 診断のエラーがないプログラムだけを受け取る。エラーがあれば `eml_cli` は Core IR を作らない
/// (docs/implementation/architecture.md)。
pub fn lower(module: &Module, typed: &TypedModule) -> Program {
    let mut program = ProgramBuilder::default();
    let mut indices = ArenaMap::default();
    for (id, function) in module.functions.iter() {
        let body = function
            .body
            .as_ref()
            .expect("a program without errors has an equation for every function");
        indices.insert(id, program.reserve(body.params.len()));
    }
    for (id, function) in module.functions.iter() {
        let body = function.body.as_ref().expect("checked above");
        let signature = typed
            .signatures
            .get(id)
            .expect("every function has a signature");
        let params = param_types(signature, body.params.len());
        let mut lambdas = 0;
        let core = FnLowering {
            module,
            body,
            types: typed.bodies.get(id).expect("every body is type-checked"),
            indices: &indices,
            program: &mut program,
            root_name: &function.name,
            lambdas: &mut lambdas,
            exprs: Vec::new(),
            vars: Vec::new(),
            locals: ArenaMap::default(),
        }
        .lower(&function.name, &[], &body.params, &params, body.root);
        program.finish(indices[id], core);
    }
    let main = typed
        .main
        .expect("`eml_cli::compile` reports a missing `main`");
    Program {
        functions: program
            .functions
            .into_iter()
            .map(|function| function.expect("every reserved function is lowered"))
            .collect(),
        main: indices[main],
        strings: program.strings.values,
    }
}

#[derive(Default)]
struct Strings {
    values: Vec<String>,
    ids: HashMap<String, u32>,
}

impl Strings {
    fn intern(&mut self, text: &str) -> u32 {
        if let Some(&id) = self.ids.get(text) {
            return id;
        }
        let id = self.values.len() as u32;
        self.values.push(text.to_string());
        self.ids.insert(text.to_string(), id);
        id
    }
}

/// 変換の途中で、ラムダと包んだ組み込みの関数を足していく関数の表。番号を先に取り、中身は変換が終わってから入れる。
#[derive(Default)]
struct ProgramBuilder {
    functions: Vec<Option<CoreFn>>,
    arities: Vec<usize>,
    strings: Strings,
    wrappers: HashMap<Builtin, FnIdx>,
}

impl ProgramBuilder {
    fn reserve(&mut self, arity: usize) -> FnIdx {
        self.functions.push(None);
        self.arities.push(arity);
        FnIdx(self.functions.len() as u32 - 1)
    }

    fn arity(&self, function: FnIdx) -> usize {
        self.arities[function.0 as usize]
    }

    fn finish(&mut self, function: FnIdx, mut core: CoreFn) {
        perceus::insert_rc(&mut core);
        self.functions[function.0 as usize] = Some(core);
    }

    /// 組み込みを値や部分適用で使うときに、それを呼ぶだけの関数を作る。組み込みごとに1つだけ作る。
    fn wrapper(&mut self, builtin: Builtin) -> FnIdx {
        if let Some(&function) = self.wrappers.get(&builtin) {
            return function;
        }
        let boxed = builtin_params(builtin);
        let function = self.reserve(boxed.len());
        self.wrappers.insert(builtin, function);
        let mut vars: Vec<VarInfo> = boxed
            .iter()
            .map(|&boxed| VarInfo {
                name: "p".to_string(),
                linearity: Linearity::Unr,
                boxed,
            })
            .collect();
        let params: Vec<VarId> = (0..boxed.len() as u32).map(VarId).collect();
        let atoms: Vec<Atom> = params.iter().map(|&param| Atom::Var(param)).collect();
        let mut fresh = |boxed: bool| {
            vars.push(VarInfo {
                name: "t".to_string(),
                linearity: Linearity::Unr,
                boxed,
            });
            VarId(vars.len() as u32 - 1)
        };
        let steps: Vec<(VarId, Rhs)> = match builtin {
            Builtin::Println => vec![(fresh(false), Rhs::Perform(IoOp::Println, atoms))],
            Builtin::ComposeFwd | Builtin::ComposeBwd => {
                // `>>` は `g (f x)`、`<<` は `f (g x)` である (docs/spec/declarations.md の演算子の表が `>>` / `<<` を関数合成としている)
                let (inner, outer) = if builtin == Builtin::ComposeFwd {
                    (atoms[0], atoms[1])
                } else {
                    (atoms[1], atoms[0])
                };
                let mid = fresh(true);
                let result = fresh(true);
                vec![
                    (mid, Rhs::Apply(inner, vec![atoms[2]])),
                    (result, Rhs::Apply(outer, vec![Atom::Var(mid)])),
                ]
            }
            other => vec![(
                fresh(builtin_result_boxed(other)),
                Rhs::Prim(prim(other), atoms),
            )],
        };
        let mut exprs = Vec::new();
        let last = steps.last().expect("every wrapper binds a result").0;
        exprs.push(CExpr::Return(Atom::Var(last)));
        let mut body = CExprId(0);
        for (var, rhs) in steps.into_iter().rev() {
            exprs.push(CExpr::Let { var, rhs, body });
            body = CExprId(exprs.len() as u32 - 1);
        }
        let core = CoreFn {
            name: format!("builtin${}", builtin.name()),
            params,
            vars,
            body,
            exprs,
        };
        self.finish(function, core);
        function
    }
}

/// 関数型の先頭の `count` 個の引数の型。
fn param_types(ty: &Type, count: usize) -> Vec<Type> {
    let mut out = Vec::new();
    let mut ty = ty;
    for _ in 0..count {
        let Type::Fn { param, ret, .. } = ty else {
            unreachable!("the type checker matched parameters with arrows");
        };
        out.push((**param).clone());
        ty = ret;
    }
    out
}

/// 引数のパターンが束縛する局所変数。型を明示した引数は内側のパターンを見る。
fn binder(body: &Body, pat: PatId) -> Option<LocalId> {
    match &body.pats[pat].kind {
        PatKind::Bind(local) => Some(*local),
        PatKind::Annot { pat, .. } => binder(body, *pat),
        _ => None,
    }
}

/// 組み込みの引数ごとの、ヒープの値かどうか。`True` と `False` は値なので、ここには来ない。
fn builtin_params(builtin: Builtin) -> Vec<bool> {
    match builtin {
        Builtin::Println => vec![true],
        Builtin::ShowInt | Builtin::Not | Builtin::IntNeg => vec![false],
        Builtin::IntAdd
        | Builtin::IntSub
        | Builtin::IntMul
        | Builtin::IntDiv
        | Builtin::IntMod
        | Builtin::IntEq
        | Builtin::IntNe
        | Builtin::IntLt
        | Builtin::IntLe
        | Builtin::IntGt
        | Builtin::IntGe => vec![false, false],
        Builtin::StrConcat => vec![true, true],
        Builtin::ComposeFwd | Builtin::ComposeBwd => vec![true, true, true],
        Builtin::True | Builtin::False => unreachable!("constructors are values, not functions"),
    }
}

fn builtin_result_boxed(builtin: Builtin) -> bool {
    matches!(builtin, Builtin::ShowInt | Builtin::StrConcat)
}

type Bindings = Vec<(VarId, Rhs)>;

struct FnLowering<'a> {
    module: &'a Module,
    body: &'a Body,
    types: &'a BodyTypes,
    indices: &'a ArenaMap<FunctionId, FnIdx>,
    program: &'a mut ProgramBuilder,
    /// ラムダの関数の名前に使う、トップレベルの関数の名前と、その中のラムダの数。
    root_name: &'a str,
    lambdas: &'a mut u32,
    exprs: Vec<CExpr>,
    vars: Vec<VarInfo>,
    locals: ArenaMap<LocalId, Atom>,
}

impl FnLowering<'_> {
    /// ラムダは捕まえた変数を先頭の引数に持つ (docs/spec/core-ir.md)。トップレベルの関数では `captured` は空である。
    fn lower(
        mut self,
        name: &str,
        captured: &[(LocalId, Type)],
        params: &[PatId],
        param_types: &[Type],
        root: ExprId,
    ) -> CoreFn {
        let body = self.body;
        let mut vars = Vec::new();
        for (local, ty) in captured {
            let var = self.new_var(&body.locals[*local].name, ty);
            self.locals.insert(*local, Atom::Var(var));
            vars.push(var);
        }
        for (&pat, ty) in params.iter().zip(param_types) {
            let local = binder(body, pat);
            let name = local.map_or("p", |local| body.locals[local].name.as_str());
            let var = self.new_var(name, ty);
            if let Some(local) = local {
                self.locals.insert(local, Atom::Var(var));
            }
            vars.push(var);
        }
        let root = self.tail(root);
        CoreFn {
            name: name.to_string(),
            params: vars,
            vars: self.vars,
            body: root,
            exprs: self.exprs,
        }
    }

    fn new_var(&mut self, name: &str, ty: &Type) -> VarId {
        self.vars.push(VarInfo {
            name: name.to_string(),
            linearity: Linearity::Unr,
            // 関数値と型変数の値は、ヒープのクロージャや文字列かもしれない。インタプリタの `dup` / `decref` は
            // ヒープにない値を無視するので、多めに対象にしても正しく動く (docs/spec/core-ir.md)
            boxed: matches!(ty, Type::String | Type::Fn { .. } | Type::Var(_)),
        });
        VarId(self.vars.len() as u32 - 1)
    }

    fn push(&mut self, expr: CExpr) -> CExprId {
        self.exprs.push(expr);
        CExprId(self.exprs.len() as u32 - 1)
    }

    fn seq(&mut self, bindings: Bindings, last: CExpr) -> CExprId {
        let mut id = self.push(last);
        for (var, rhs) in bindings.into_iter().rev() {
            id = self.push(CExpr::Let { var, rhs, body: id });
        }
        id
    }

    /// 式の値を返すコード。
    fn tail(&mut self, expr: ExprId) -> CExprId {
        let mut bindings = Vec::new();
        let atom = self.atom(expr, &mut bindings);
        self.seq(bindings, CExpr::Return(atom))
    }

    fn ty(&self, expr: ExprId) -> Type {
        self.types
            .exprs
            .get(expr)
            .cloned()
            .expect("every reached expression is typed")
    }

    fn bind(&mut self, out: &mut Bindings, name: &str, ty: &Type, rhs: Rhs) -> Atom {
        let var = self.new_var(name, ty);
        out.push((var, rhs));
        Atom::Var(var)
    }

    /// 型を見ずに、ヒープの値として変数を作る。クロージャと、関数値を返す途中の結果に使う。
    fn bind_boxed(&mut self, out: &mut Bindings, name: &str, rhs: Rhs) -> Atom {
        self.vars.push(VarInfo {
            name: name.to_string(),
            linearity: Linearity::Unr,
            boxed: true,
        });
        let var = VarId(self.vars.len() as u32 - 1);
        out.push((var, rhs));
        Atom::Var(var)
    }

    fn atoms(&mut self, exprs: &[ExprId], out: &mut Bindings) -> Vec<Atom> {
        exprs.iter().map(|&expr| self.atom(expr, out)).collect()
    }

    /// 呼ぶ相手の引数の個数と比べ、揃えば直接呼び、足りなければクロージャにし、余れば戻った関数値に残りを適用する
    /// (docs/spec/core-ir.md の eval/apply)。
    fn call_known(
        &mut self,
        target: FnIdx,
        mut args: Vec<Atom>,
        ty: &Type,
        out: &mut Bindings,
    ) -> Atom {
        let arity = self.program.arity(target);
        if args.len() < arity {
            return self.bind_boxed(out, "c", Rhs::MakeClosure(target, args));
        }
        let rest = args.split_off(arity);
        if rest.is_empty() {
            return self.bind(out, "t", ty, Rhs::CallDirect(target, args));
        }
        let function = self.bind_boxed(out, "t", Rhs::CallDirect(target, args));
        self.bind(out, "t", ty, Rhs::Apply(function, rest))
    }

    fn call_builtin(
        &mut self,
        builtin: Builtin,
        mut args: Vec<Atom>,
        ty: &Type,
        out: &mut Bindings,
    ) -> Atom {
        let arity = builtin_params(builtin).len();
        if args.len() < arity {
            let wrapper = self.program.wrapper(builtin);
            return self.bind_boxed(out, "c", Rhs::MakeClosure(wrapper, args));
        }
        let rest = args.split_off(arity);
        let rhs = match builtin {
            Builtin::Println => Rhs::Perform(IoOp::Println, args),
            Builtin::ComposeFwd | Builtin::ComposeBwd => {
                Rhs::CallDirect(self.program.wrapper(builtin), args)
            }
            other => Rhs::Prim(prim(other), args),
        };
        if rest.is_empty() {
            return self.bind(out, "t", ty, rhs);
        }
        let function = self.bind_boxed(out, "t", rhs);
        self.bind(out, "t", ty, Rhs::Apply(function, rest))
    }

    /// 式の値をアトムにする。値の計算に要る束縛は `out` に積む。
    fn atom(&mut self, id: ExprId, out: &mut Bindings) -> Atom {
        let body = self.body;
        match &body.exprs[id].kind {
            ExprKind::Missing => {
                unreachable!("a program without errors has no missing expressions")
            }
            ExprKind::Literal(Literal::Int(n)) => Atom::Int(*n),
            ExprKind::Literal(Literal::Unit) => Atom::Unit,
            ExprKind::Literal(Literal::String(text)) => {
                let index = self.program.strings.intern(text);
                self.bind(out, "s", &Type::String, Rhs::ConstString(index))
            }
            ExprKind::Path(Res::Local(local)) => self.locals[*local],
            ExprKind::Path(Res::Builtin(Builtin::True)) => Atom::Tag(TRUE),
            ExprKind::Path(Res::Builtin(Builtin::False)) => Atom::Tag(FALSE),
            ExprKind::Path(Res::Builtin(builtin)) => {
                let wrapper = self.program.wrapper(*builtin);
                self.bind_boxed(out, "c", Rhs::MakeClosure(wrapper, Vec::new()))
            }
            // 引数のないトップレベルの値は、参照するたびに呼び出す (docs/spec/core-ir.md)
            ExprKind::Path(Res::Function(function)) => {
                let target = self.indices[*function];
                if self.program.arity(target) == 0 {
                    let ty = self.ty(id);
                    let name = self.module.functions[*function].name.clone();
                    self.bind(out, &name, &ty, Rhs::CallDirect(target, Vec::new()))
                } else {
                    self.bind_boxed(out, "c", Rhs::MakeClosure(target, Vec::new()))
                }
            }
            ExprKind::Call { callee, args } => {
                let ty = self.ty(id);
                match &body.exprs[*callee].kind {
                    // 引数のない値の参照は呼び出しなので、呼ばれる式として先に評価する必要がある。一般の経路に回す
                    ExprKind::Path(Res::Function(function))
                        if self.program.arity(self.indices[*function]) > 0 =>
                    {
                        let args = self.atoms(args, out);
                        let target = self.indices[*function];
                        self.call_known(target, args, &ty, out)
                    }
                    ExprKind::Path(Res::Builtin(builtin)) => {
                        let args = self.atoms(args, out);
                        self.call_builtin(*builtin, args, &ty, out)
                    }
                    _ => {
                        // 呼ばれる式は引数より左にあるので、先に評価する
                        let function = self.atom(*callee, out);
                        let args = self.atoms(args, out);
                        self.bind(out, "t", &ty, Rhs::Apply(function, args))
                    }
                }
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let scrutinee = self.atom(*condition, out);
                let then_code = self.tail(*then_branch);
                let else_code = match else_branch {
                    Some(else_branch) => self.tail(*else_branch),
                    None => self.push(CExpr::Return(Atom::Unit)),
                };
                let switch = self.push(CExpr::Switch {
                    scrutinee,
                    arms: vec![(FALSE, else_code), (TRUE, then_code)],
                });
                let ty = self.ty(id);
                self.bind(out, "t", &ty, Rhs::Nested(switch))
            }
            ExprKind::Block { stmts, tail } => {
                for stmt in stmts {
                    match stmt {
                        Stmt::Let { pat, init, .. } => {
                            let value = self.atom(*init, out);
                            self.bind_pat(*pat, value);
                        }
                        // 式文の値は `Unit` なので捨ててよい
                        Stmt::Expr(expr) => {
                            self.atom(*expr, out);
                        }
                    }
                }
                match tail {
                    Some(tail) => self.atom(*tail, out),
                    None => Atom::Unit,
                }
            }
            ExprKind::Annot { expr, .. } => self.atom(*expr, out),
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                let captured: Vec<(LocalId, Type)> = captures(body, id)
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
                let lambda_ty = self.ty(id);
                let param_types = param_types(&lambda_ty, params.len());
                let function = self.program.reserve(captured.len() + params.len());
                let name = format!("{}$lambda{}", self.root_name, *self.lambdas);
                *self.lambdas += 1;
                let core = FnLowering {
                    module: self.module,
                    body,
                    types: self.types,
                    indices: self.indices,
                    program: &mut *self.program,
                    root_name: self.root_name,
                    lambdas: &mut *self.lambdas,
                    exprs: Vec::new(),
                    vars: Vec::new(),
                    locals: ArenaMap::default(),
                }
                .lower(&name, &captured, params, &param_types, *lambda_body);
                self.program.finish(function, core);
                let atoms = captured
                    .iter()
                    .map(|(local, _)| self.locals[*local])
                    .collect();
                self.bind_boxed(out, "c", Rhs::MakeClosure(function, atoms))
            }
        }
    }

    /// `_` と `()` で受けた値は以後使われないので、Perceus の挿入が decref する。
    fn bind_pat(&mut self, pat: PatId, value: Atom) {
        match self.body.pats[pat].kind {
            PatKind::Bind(local) => {
                self.locals.insert(local, value);
            }
            PatKind::Annot { pat, .. } => self.bind_pat(pat, value),
            _ => {}
        }
    }
}

fn prim(builtin: Builtin) -> PrimOp {
    match builtin {
        Builtin::IntAdd => PrimOp::IntAdd,
        Builtin::IntSub => PrimOp::IntSub,
        Builtin::IntMul => PrimOp::IntMul,
        Builtin::IntDiv => PrimOp::IntDiv,
        Builtin::IntMod => PrimOp::IntMod,
        Builtin::IntNeg => PrimOp::IntNeg,
        Builtin::IntEq => PrimOp::IntEq,
        Builtin::IntNe => PrimOp::IntNe,
        Builtin::IntLt => PrimOp::IntLt,
        Builtin::IntLe => PrimOp::IntLe,
        Builtin::IntGt => PrimOp::IntGt,
        Builtin::IntGe => PrimOp::IntGe,
        Builtin::StrConcat => PrimOp::StrConcat,
        Builtin::ShowInt => PrimOp::ShowInt,
        Builtin::Not => PrimOp::Not,
        Builtin::Println | Builtin::True | Builtin::False => {
            unreachable!("`println` is performed and constructors are values")
        }
        Builtin::ComposeFwd | Builtin::ComposeBwd => {
            unreachable!("composition is lowered to a closure, not to a primitive")
        }
    }
}

/// ラムダの本体が参照する、ラムダの外で束縛した局所変数。`LocalId` の順に並べる。ラムダは捕まえた変数を先頭の
/// 引数に持つ関数に持ち上げる (docs/spec/core-ir.md)。そのため、入れ子のラムダが捕まえる変数は、内側のクロージャを
/// 作る外側のラムダの関数でも引数として要るので、外側のラムダも捕まえる。式の木は作業リストでたどる。
fn captures(body: &Body, lambda: ExprId) -> Vec<LocalId> {
    let mut used = BTreeSet::new();
    let mut bound = HashSet::new();
    let mut work = vec![lambda];
    while let Some(id) = work.pop() {
        match &body.exprs[id].kind {
            ExprKind::Path(Res::Local(local)) => {
                used.insert(*local);
            }
            ExprKind::Lambda {
                params,
                body: inner,
            } => {
                for &param in params {
                    bind_locals(body, param, &mut bound);
                }
                work.push(*inner);
            }
            ExprKind::Call { callee, args } => {
                work.push(*callee);
                work.extend(args.iter().copied());
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                work.push(*condition);
                work.push(*then_branch);
                work.extend(*else_branch);
            }
            ExprKind::Block { stmts, tail } => {
                for stmt in stmts {
                    match stmt {
                        Stmt::Let { pat, init, .. } => {
                            bind_locals(body, *pat, &mut bound);
                            work.push(*init);
                        }
                        Stmt::Expr(expr) => work.push(*expr),
                    }
                }
                work.extend(*tail);
            }
            ExprKind::Annot { expr, .. } => work.push(*expr),
            ExprKind::Missing | ExprKind::Literal(_) | ExprKind::Path(_) => {}
        }
    }
    used.into_iter()
        .filter(|local| !bound.contains(local))
        .collect()
}

fn bind_locals(body: &Body, pat: PatId, bound: &mut HashSet<LocalId>) {
    match &body.pats[pat].kind {
        PatKind::Bind(local) => {
            bound.insert(*local);
        }
        PatKind::Annot { pat, .. } => bind_locals(body, *pat, bound),
        PatKind::Wildcard | PatKind::Unit | PatKind::Missing => {}
    }
}
