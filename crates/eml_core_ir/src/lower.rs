use std::collections::HashMap;

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
    let mut indices = ArenaMap::default();
    for (index, (id, _)) in module.functions.iter().enumerate() {
        indices.insert(id, FnIdx(index as u32));
    }
    let mut strings = Strings::default();
    let mut functions = Vec::new();
    for (id, function) in module.functions.iter() {
        let body = function
            .body
            .as_ref()
            .expect("a program without errors has an equation for every function");
        let lowering = FnLowering {
            module,
            body,
            types: typed.bodies.get(id).expect("every body is type-checked"),
            indices: &indices,
            strings: &mut strings,
            exprs: Vec::new(),
            vars: Vec::new(),
            locals: ArenaMap::default(),
        };
        let signature = typed
            .signatures
            .get(id)
            .expect("every function has a signature");
        let mut core = lowering.lower(&function.name, signature);
        perceus::insert_rc(&mut core);
        functions.push(core);
    }
    let main = typed
        .main
        .expect("`eml_cli::compile` reports a missing `main`");
    Program {
        functions,
        main: indices[main],
        strings: strings.values,
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

type Bindings = Vec<(VarId, Rhs)>;

struct FnLowering<'a> {
    module: &'a Module,
    body: &'a Body,
    types: &'a BodyTypes,
    indices: &'a ArenaMap<FunctionId, FnIdx>,
    strings: &'a mut Strings,
    exprs: Vec<CExpr>,
    vars: Vec<VarInfo>,
    locals: ArenaMap<LocalId, Atom>,
}

impl FnLowering<'_> {
    fn lower(mut self, name: &str, signature: &Type) -> CoreFn {
        let body = self.body;
        let mut params = Vec::new();
        let mut ty = signature;
        for &pat in &body.params {
            let Type::Fn { param, ret, .. } = ty else {
                unreachable!("the type checker matched parameters with arrows");
            };
            let name = match body.pats[pat].kind {
                PatKind::Bind(local) => body.locals[local].name.clone(),
                _ => "p".to_string(),
            };
            let var = self.new_var(&name, param);
            if let PatKind::Bind(local) = body.pats[pat].kind {
                self.locals.insert(local, Atom::Var(var));
            }
            params.push(var);
            ty = ret.as_ref();
        }
        let root = self.tail(body.root);
        CoreFn {
            name: name.to_string(),
            params,
            vars: self.vars,
            body: root,
            exprs: self.exprs,
        }
    }

    fn new_var(&mut self, name: &str, ty: &Type) -> VarId {
        self.vars.push(VarInfo {
            name: name.to_string(),
            // 段階1の値はすべて `Unr` で、ヒープに置くのは文字列だけである
            linearity: Linearity::Unr,
            boxed: matches!(ty, Type::String),
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
                let index = self.strings.intern(text);
                self.bind(out, "s", &Type::String, Rhs::ConstString(index))
            }
            ExprKind::Path(Res::Local(local)) => self.locals[*local],
            ExprKind::Path(Res::Builtin(Builtin::True)) => Atom::Tag(TRUE),
            ExprKind::Path(Res::Builtin(Builtin::False)) => Atom::Tag(FALSE),
            ExprKind::Path(Res::Builtin(_)) => {
                unreachable!("the type checker allows builtin functions only as callees")
            }
            // 引数のないトップレベルの値は、参照するたびに呼び出す
            ExprKind::Path(Res::Function(function)) => {
                let ty = self.ty(id);
                let name = self.module.functions[*function].name.clone();
                let callee = self.indices[*function];
                self.bind(out, &name, &ty, Rhs::CallDirect(callee, Vec::new()))
            }
            ExprKind::Call { callee, args } => {
                let args: Vec<Atom> = args.iter().map(|&arg| self.atom(arg, out)).collect();
                let rhs = match &body.exprs[*callee].kind {
                    ExprKind::Path(Res::Function(function)) => {
                        Rhs::CallDirect(self.indices[*function], args)
                    }
                    ExprKind::Path(Res::Builtin(Builtin::Println)) => {
                        Rhs::Perform(IoOp::Println, args)
                    }
                    ExprKind::Path(Res::Builtin(builtin)) => Rhs::Prim(prim(*builtin), args),
                    _ => unreachable!("only functions and builtins are called in stage 1"),
                };
                let ty = self.ty(id);
                self.bind(out, "t", &ty, rhs)
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
            ExprKind::Lambda { .. } => {
                unreachable!("the type checker rejects lambdas until they are lowered")
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
