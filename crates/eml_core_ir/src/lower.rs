use std::collections::HashMap;

use eml_hir::builtin::Builtin;
use eml_hir::{
    Body, ExprId, ExprKind, FunctionId, LangItems, Literal, LocalId, Module, PatId, Res, Stmt,
};
use eml_types::{BodyTypes, Linearity, Type, TypedModule};
use la_arena::ArenaMap;

use crate::{
    Atom, CExpr, CExprId, Call, CoreFn, FALSE, FnIdx, IoOp, JoinId, PrimOp, Program, Rhs, TRUE,
    VarId, VarInfo, perceus, saved,
};

/// 診断のエラーがないプログラムだけを受け取る。エラーがあれば `eml_cli` は Core IR を作らない
/// (docs/implementation/architecture.md)。
pub fn lower(module: &Module, typed: &TypedModule) -> Program {
    let mut builder = ProgramBuilder::new(module, typed);
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
        let (params, _) = split_arrows(signature, body.params.len());
        let mut lambdas = 0;
        let core = FnLowering {
            module,
            body,
            types: typed.bodies.get(id).expect("every body is type-checked"),
            indices: &indices,
            program: &mut builder,
            root_name: &function.name,
            lambdas: &mut lambdas,
            exprs: Vec::new(),
            vars: Vec::new(),
            locals: ArenaMap::default(),
            joins: Vec::new(),
        }
        .lower(&function.name, &[], &body.params, &params, body.root);
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
    let entry = builder.entry(indices[main], main_type);
    let mut program = Program {
        functions: builder
            .functions
            .into_iter()
            .map(|function| function.expect("every reserved function is lowered"))
            .collect(),
        entry,
        strings: builder.strings.values,
    };
    perceus::insert(&mut program);
    saved::record(&mut program);
    // Perceus の誤りを、実行した経路だけでなく変換のたびに見つける (docs/spec/core-ir.md)
    #[cfg(debug_assertions)]
    if let Err(error) = crate::verify(&program) {
        panic!("internal error: invalid Core IR: {error}");
    }
    program
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
struct ProgramBuilder {
    lang: LangItems,
    /// Prelude から作った組み込みの型。組み込みを包む関数の変数が boxed かどうかを決める。
    builtin_types: HashMap<Builtin, Type>,
    functions: Vec<Option<CoreFn>>,
    arities: Vec<usize>,
    strings: Strings,
    wrappers: HashMap<Builtin, FnIdx>,
}

impl ProgramBuilder {
    fn new(module: &Module, typed: &TypedModule) -> ProgramBuilder {
        ProgramBuilder {
            lang: module.lang,
            builtin_types: typed
                .builtins
                .iter()
                .map(|(&builtin, scheme)| (builtin, scheme.ty.clone()))
                .collect(),
            functions: Vec::new(),
            arities: Vec::new(),
            strings: Strings::default(),
            wrappers: HashMap::new(),
        }
    }

    fn reserve(&mut self, arity: usize) -> FnIdx {
        self.functions.push(None);
        self.arities.push(arity);
        FnIdx(self.functions.len() as u32 - 1)
    }

    fn arity(&self, function: FnIdx) -> usize {
        self.arities[function.0 as usize]
    }

    fn finish(&mut self, function: FnIdx, core: CoreFn) {
        self.functions[function.0 as usize] = Some(core);
    }

    /// 実行の入口。`main : Unit -> <IO> Unit` を `()` で呼ぶ。等式に引数のない `main = fn () -> ...` は関数値を返す
    /// ので、返った値に `()` を適用する (docs/spec/core-ir.md)。
    fn entry(&mut self, main: FnIdx, main_type: &Type) -> FnIdx {
        let function = self.reserve(0);
        let unit = vec![Atom::Unit];
        let (vars, exprs) = if self.arity(main) == 0 {
            let value = VarId(0);
            (
                vec![var_info("f", main_type, &self.lang)],
                vec![
                    CExpr::TailCall(Call::Apply(Atom::Var(value), unit)),
                    CExpr::Let {
                        var: value,
                        rhs: Rhs::call(Call::Direct(main, Vec::new())),
                        body: CExprId(0),
                    },
                ],
            )
        } else {
            (Vec::new(), vec![CExpr::TailCall(Call::Direct(main, unit))])
        };
        let core = CoreFn {
            name: "entry$main".to_string(),
            params: Vec::new(),
            vars,
            body: CExprId(exprs.len() as u32 - 1),
            exprs,
            joins: Vec::new(),
        };
        self.finish(function, core);
        function
    }

    /// 組み込みを値や部分適用で使うときに、それを呼ぶだけの関数を作る。組み込みごとに1つだけ作る。
    fn wrapper(&mut self, builtin: Builtin) -> FnIdx {
        if let Some(&function) = self.wrappers.get(&builtin) {
            return function;
        }
        let arity = builtin.arity();
        let ty = self
            .builtin_types
            .get(&builtin)
            .expect("every builtin function has a Prelude signature");
        let (param_types, result_type) = split_arrows(ty, arity);
        let lang = self.lang;
        let function = self.reserve(arity);
        self.wrappers.insert(builtin, function);
        let mut vars: Vec<VarInfo> = param_types
            .iter()
            .map(|ty| var_info("p", ty, &lang))
            .collect();
        let params: Vec<VarId> = (0..arity as u32).map(VarId).collect();
        let atoms: Vec<Atom> = params.iter().map(|&param| Atom::Var(param)).collect();
        let mut fresh = |ty: &Type| {
            vars.push(var_info("t", ty, &lang));
            VarId(vars.len() as u32 - 1)
        };
        let (steps, last): (Vec<(VarId, Rhs)>, CExpr) = match lowering(builtin) {
            Lowering::Prim(op) => {
                let result = fresh(&result_type);
                (
                    vec![(result, Rhs::Prim(op, atoms))],
                    CExpr::Return(Atom::Var(result)),
                )
            }
            Lowering::Perform(op) => {
                let result = fresh(&result_type);
                (
                    vec![(result, Rhs::Perform(op, atoms))],
                    CExpr::Return(Atom::Var(result)),
                )
            }
            Lowering::Compose { forward } => {
                let (inner, outer) = if forward { (0, 1) } else { (1, 0) };
                let (_, middle_type) = split_arrows(&param_types[inner], 1);
                let middle = fresh(&middle_type);
                (
                    vec![(middle, Rhs::call(Call::Apply(atoms[inner], vec![atoms[2]])))],
                    CExpr::TailCall(Call::Apply(atoms[outer], vec![Atom::Var(middle)])),
                )
            }
            Lowering::Constructor(_) => unreachable!("constructors are values, not functions"),
        };
        let mut exprs = vec![last];
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
            joins: Vec::new(),
        };
        self.finish(function, core);
        function
    }
}

/// ヒープに置く値の型。`Unr` でボックス化した変数が RC の対象になる。関数値と型変数の値は、ヒープのクロージャや
/// 文字列かもしれない。インタプリタの `dup` / `decref` はヒープにない値を無視するので、多めに対象にしても正しく動く
/// (docs/spec/core-ir.md)。
fn boxed(ty: &Type, lang: &LangItems) -> bool {
    match ty {
        Type::Con { id, .. } => *id == lang.string,
        Type::Fn { .. } | Type::Cont { .. } | Type::Rigid(_) | Type::Flexible => true,
        Type::Record(_) | Type::Error => false,
    }
}

fn var_info(name: &str, ty: &Type, lang: &LangItems) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        linearity: Linearity::Unr,
        boxed: boxed(ty, lang),
    }
}

/// 関数型の先頭の `count` 個の引数の型と、残りの型。
fn split_arrows(ty: &Type, count: usize) -> (Vec<Type>, Type) {
    let mut params = Vec::new();
    let mut ty = ty;
    for _ in 0..count {
        let Type::Fn { param, ret, .. } = ty else {
            unreachable!("the type checker matched parameters with arrows");
        };
        params.push((**param).clone());
        ty = ret;
    }
    (params, ty.clone())
}

/// 組み込みを Core IR のどの命令にするか。引数の数は `Builtin::arity` (eml_hir の表) から、引数と結果の型は Prelude の
/// スキームから引くので、ここには変換の種類だけを置く。
enum Lowering {
    Prim(PrimOp),
    Perform(IoOp),
    /// `>>` は `g (f x)`、`<<` は `f (g x)` である (docs/spec/declarations.md の演算子の表)。
    Compose {
        forward: bool,
    },
    Constructor(u32),
}

fn lowering(builtin: Builtin) -> Lowering {
    match builtin {
        Builtin::Println => Lowering::Perform(IoOp::Println),
        Builtin::ShowInt => Lowering::Prim(PrimOp::ShowInt),
        Builtin::Not => Lowering::Prim(PrimOp::Not),
        Builtin::IntNeg => Lowering::Prim(PrimOp::IntNeg),
        Builtin::IntAdd => Lowering::Prim(PrimOp::IntAdd),
        Builtin::IntSub => Lowering::Prim(PrimOp::IntSub),
        Builtin::IntMul => Lowering::Prim(PrimOp::IntMul),
        Builtin::IntDiv => Lowering::Prim(PrimOp::IntDiv),
        Builtin::IntMod => Lowering::Prim(PrimOp::IntMod),
        Builtin::IntEq => Lowering::Prim(PrimOp::IntEq),
        Builtin::IntNe => Lowering::Prim(PrimOp::IntNe),
        Builtin::IntLt => Lowering::Prim(PrimOp::IntLt),
        Builtin::IntLe => Lowering::Prim(PrimOp::IntLe),
        Builtin::IntGt => Lowering::Prim(PrimOp::IntGt),
        Builtin::IntGe => Lowering::Prim(PrimOp::IntGe),
        Builtin::StrConcat => Lowering::Prim(PrimOp::StrConcat),
        Builtin::ComposeFwd => Lowering::Compose { forward: true },
        Builtin::ComposeBwd => Lowering::Compose { forward: false },
        Builtin::True => Lowering::Constructor(TRUE),
        Builtin::False => Lowering::Constructor(FALSE),
    }
}

/// 値を計算する束縛と、join point の開始の並び。`seq` が後ろから組み立てる。
enum Binding {
    Let(VarId, Rhs),
    /// ここより後ろで組み立てる式を本体にし、`scope` (枝が `Jump` する `Switch`) を範囲にする join point。
    Join {
        join: JoinId,
        param: VarId,
        scope: CExprId,
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
        Exit::Jump(join) => CExpr::Jump { join, arg: value },
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
    exprs: Vec<CExpr>,
    vars: Vec<VarInfo>,
    locals: ArenaMap<LocalId, Atom>,
    /// `JoinId` から `Join` の式への索引。`seq` が join point を組み立てたときに埋める。
    joins: Vec<Option<CExprId>>,
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
            let local = body.pat_bindings(pat).first().copied();
            let name = local.map_or("p", |local| body.locals[local].name.as_str());
            let var = self.new_var(name, ty);
            if let Some(local) = local {
                self.locals.insert(local, Atom::Var(var));
            }
            vars.push(var);
        }
        let root = self.tail(root, Exit::Return);
        CoreFn {
            name: name.to_string(),
            params: vars,
            vars: self.vars,
            body: root,
            exprs: self.exprs,
            joins: self
                .joins
                .into_iter()
                .map(|join| join.expect("every join point is built"))
                .collect(),
        }
    }

    fn new_var(&mut self, name: &str, ty: &Type) -> VarId {
        self.vars.push(var_info(name, ty, &self.module.lang));
        VarId(self.vars.len() as u32 - 1)
    }

    fn push(&mut self, expr: CExpr) -> CExprId {
        self.exprs.push(expr);
        CExprId(self.exprs.len() as u32 - 1)
    }

    fn seq(&mut self, bindings: Bindings, last: CExpr) -> CExprId {
        let mut id = self.push(last);
        for binding in bindings.into_iter().rev() {
            id = match binding {
                Binding::Let(var, rhs) => self.push(CExpr::Let { var, rhs, body: id }),
                Binding::Join { join, param, scope } => {
                    let expr = self.push(CExpr::Join {
                        join,
                        param,
                        body: id,
                        scope,
                    });
                    self.joins[join.0 as usize] = Some(expr);
                    expr
                }
            };
        }
        id
    }

    /// 式の値を `exit` に渡すコード。値を返すだけの呼び出しは、呼び出し元のフレームを積まない末尾呼び出しにする。
    fn tail(&mut self, expr: ExprId, exit: Exit) -> CExprId {
        let mut bindings = Vec::new();
        let mut last = self.tail_expr(expr, exit, &mut bindings);
        if let CExpr::Return(Atom::Var(returned)) = last
            && let Some(Binding::Let(bound, Rhs::Call { .. })) = bindings.last()
            && *bound == returned
        {
            let Some(Binding::Let(_, Rhs::Call { call, .. })) = bindings.pop() else {
                unreachable!("checked above");
            };
            // 結果の変数は呼び出しの直前に作ったものなので、表から除いて番号を詰める
            debug_assert_eq!(returned.0 as usize, self.vars.len() - 1);
            self.vars.pop();
            last = CExpr::TailCall(call);
        }
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
                    arms: vec![(FALSE, else_code), (TRUE, then_code)],
                }
            }
            ExprKind::Block { stmts, tail } => {
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
                    self.bind_pat(*pat, value);
                }
                // 式文の値は `Unit` なので捨ててよい
                Stmt::Expr(expr) => {
                    self.atom(*expr, out);
                }
            }
        }
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
        out.push(Binding::Let(var, rhs));
        Atom::Var(var)
    }

    /// 呼ぶ相手の引数の個数と比べ、揃えば直接呼び、足りなければクロージャにし、余れば戻った関数値に残りを適用する
    /// (docs/spec/core-ir.md の eval/apply)。
    fn call_known(
        &mut self,
        target: FnIdx,
        callee_ty: &Type,
        mut args: Vec<Atom>,
        ty: &Type,
        out: &mut Bindings,
    ) -> Atom {
        let arity = self.program.arity(target);
        if args.len() < arity {
            return self.bind(out, "c", ty, Rhs::MakeClosure(target, args));
        }
        let rest = args.split_off(arity);
        if rest.is_empty() {
            return self.bind(out, "t", ty, Rhs::call(Call::Direct(target, args)));
        }
        let (_, function_ty) = split_arrows(callee_ty, arity);
        let function = self.bind(
            out,
            "t",
            &function_ty,
            Rhs::call(Call::Direct(target, args)),
        );
        self.bind(out, "t", ty, Rhs::call(Call::Apply(function, rest)))
    }

    fn call_builtin(
        &mut self,
        builtin: Builtin,
        callee_ty: &Type,
        mut args: Vec<Atom>,
        ty: &Type,
        out: &mut Bindings,
    ) -> Atom {
        let arity = builtin.arity();
        if args.len() < arity {
            let wrapper = self.program.wrapper(builtin);
            return self.bind(out, "c", ty, Rhs::MakeClosure(wrapper, args));
        }
        let rest = args.split_off(arity);
        let rhs = match lowering(builtin) {
            Lowering::Prim(op) => Rhs::Prim(op, args),
            Lowering::Perform(op) => Rhs::Perform(op, args),
            Lowering::Compose { .. } => {
                Rhs::call(Call::Direct(self.program.wrapper(builtin), args))
            }
            Lowering::Constructor(_) => unreachable!("constructors are values, not functions"),
        };
        if rest.is_empty() {
            return self.bind(out, "t", ty, rhs);
        }
        let (_, function_ty) = split_arrows(callee_ty, arity);
        let function = self.bind(out, "t", &function_ty, rhs);
        self.bind(out, "t", ty, Rhs::call(Call::Apply(function, rest)))
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
                let string = self.module.lang.string;
                let ty = Type::Con {
                    id: string,
                    name: self.module.types[string].name.clone(),
                };
                self.bind(out, "s", &ty, Rhs::ConstString(index))
            }
            ExprKind::Path(Res::Local(local)) => self.locals[*local],
            ExprKind::Path(Res::Builtin(builtin)) => match lowering(*builtin) {
                Lowering::Constructor(tag) => Atom::Tag(tag),
                _ => {
                    let wrapper = self.program.wrapper(*builtin);
                    let ty = self.ty(id);
                    self.bind(out, "c", &ty, Rhs::MakeClosure(wrapper, Vec::new()))
                }
            },
            // 引数のないトップレベルの値は、参照するたびに呼び出す (docs/spec/core-ir.md)
            ExprKind::Path(Res::Function(function)) => {
                let target = self.indices[*function];
                if self.program.arity(target) == 0 {
                    let ty = self.ty(id);
                    let name = self.module.functions[*function].name.clone();
                    self.bind(out, &name, &ty, Rhs::call(Call::Direct(target, Vec::new())))
                } else {
                    let ty = self.ty(id);
                    self.bind(out, "c", &ty, Rhs::MakeClosure(target, Vec::new()))
                }
            }
            ExprKind::Path(Res::Operation(_)) => {
                unreachable!("operations are not lowered to Core IR yet")
            }
            ExprKind::Call {
                callee,
                args,
                evaluate_first,
            } => {
                let ty = self.ty(id);
                let callee_ty = self.ty(*callee);
                // `x |> f a` の `x` は、呼ばれる式とほかの引数より先に評価する (docs/spec/declarations.md)
                let first = evaluate_first.map(|index| (index, self.atom(args[index], out)));
                match &body.exprs[*callee].kind {
                    // 引数のない値の参照は呼び出しなので、呼ばれる式として先に評価する必要がある。一般の経路に回す
                    ExprKind::Path(Res::Function(function))
                        if self.program.arity(self.indices[*function]) > 0 =>
                    {
                        let args = self.call_args(args, first, out);
                        let target = self.indices[*function];
                        self.call_known(target, &callee_ty, args, &ty, out)
                    }
                    ExprKind::Path(Res::Builtin(builtin)) => {
                        let args = self.call_args(args, first, out);
                        self.call_builtin(*builtin, &callee_ty, args, &ty, out)
                    }
                    _ => {
                        // 呼ばれる式は引数より左にあるので、先に評価する
                        let function = self.atom(*callee, out);
                        let args = self.call_args(args, first, out);
                        self.bind(out, "t", &ty, Rhs::call(Call::Apply(function, args)))
                    }
                }
            }
            ExprKind::If { .. } => {
                // 続きの式を join point の本体にし、`if` の値をその引数で受ける
                let join = JoinId(self.joins.len() as u32);
                self.joins.push(None);
                let last = self.tail_expr(id, Exit::Jump(join), out);
                let scope = self.push(last);
                let ty = self.ty(id);
                let param = self.new_var("t", &ty);
                out.push(Binding::Join { join, param, scope });
                Atom::Var(param)
            }
            ExprKind::Block { stmts, tail } => {
                self.stmts(stmts, out);
                match tail {
                    Some(tail) => self.atom(*tail, out),
                    None => Atom::Unit,
                }
            }
            ExprKind::Annot { expr, .. } => self.atom(*expr, out),
            ExprKind::Handle { .. } | ExprKind::Resume { .. } | ExprKind::Drop(_) => {
                unreachable!("handlers are not lowered to Core IR yet")
            }
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                let captured: Vec<(LocalId, Type)> = body
                    .lambda_captures(id)
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
                let (param_types, _) = split_arrows(&lambda_ty, params.len());
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
                    joins: Vec::new(),
                }
                .lower(&name, &captured, params, &param_types, *lambda_body);
                self.program.finish(function, core);
                let atoms = captured
                    .iter()
                    .map(|(local, _)| self.locals[*local])
                    .collect();
                self.bind(out, "c", &lambda_ty, Rhs::MakeClosure(function, atoms))
            }
        }
    }

    /// 引数を左から順に atom にする。`first` (位置と、先に評価した atom) の引数は評価し直さない。
    fn call_args(
        &mut self,
        args: &[ExprId],
        first: Option<(usize, Atom)>,
        out: &mut Bindings,
    ) -> Vec<Atom> {
        let mut atoms = Vec::with_capacity(args.len());
        for (index, &arg) in args.iter().enumerate() {
            match first {
                Some((first, atom)) if first == index => atoms.push(atom),
                _ => atoms.push(self.atom(arg, out)),
            }
        }
        atoms
    }

    /// `_` と `()` で受けた値は以後使われないので、Perceus の挿入が decref する。
    fn bind_pat(&mut self, pat: PatId, value: Atom) {
        for local in self.body.pat_bindings(pat) {
            self.locals.insert(local, value);
        }
    }
}
