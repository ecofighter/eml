//! 式ごとの変換と、呼び出しの引数の個数による場合分け (docs/spec/core-ir.md の eval/apply)。

use eml_hir::EvalStep;
use eml_hir::{
    Closure, ConstructorId, ExprId, ExprKind, FunctionId, Literal, OperationId, PatId, Res,
    TypeDefId,
};
use eml_types::Type;

use crate::{Atom, Call, FnIdx, Rhs, TUPLE};

use super::program::{effect_index, operation_rhs};
use super::types::{Lowering, equality_op, intrinsic, split_arrows};
use super::{Binding, Bindings, Exit, FnLowering};

/// 既知の呼ばれる式の種類。`saturate` は、引数の数、足りないときの包む関数、ちょうどのときの命令を、この種類から決める
/// (docs/implementation/architecture.md の「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」)。
#[derive(Clone, Copy)]
enum Callee {
    /// 本体のある関数。足りないときの包む関数は、その関数自身である。
    Function(FnIdx),
    /// intrinsic。`callee` は呼ばれる式で、`==` と `!=` の比べ方を `BodyTypes::equalities` から引くのに使う。
    Intrinsic {
        function: FunctionId,
        callee: ExprId,
    },
    Operation(OperationId),
    Constructor(ConstructorId),
}

impl FnLowering<'_> {
    pub(super) fn ty(&self, expr: ExprId) -> Type {
        self.types
            .exprs
            .get(expr)
            .cloned()
            .expect("every reached expression is typed")
    }

    /// 組み込みの型 (`String`、`Bool`) の、引数のない型構成子の型。
    pub(super) fn lang_type(&self, id: TypeDefId) -> Type {
        Type::Con {
            id,
            args: Vec::new(),
        }
    }

    pub(super) fn bind(&mut self, out: &mut Bindings, name: &str, ty: &Type, rhs: Rhs) -> Atom {
        let var = self.new_var(name, ty);
        out.push(Binding::Let(var, rhs));
        Atom::Var(var)
    }

    /// 呼ぶ相手の引数の個数と比べ、揃えば命令にし、足りなければ包む関数のクロージャにし、余れば命令の結果に残りを
    /// 適用する (docs/spec/core-ir.md の eval/apply)。呼ばれる式の種類によらず、この1か所で場合分けする。
    fn saturate(
        &mut self,
        callee: Callee,
        callee_ty: &Type,
        mut args: Vec<Atom>,
        ty: &Type,
        out: &mut Bindings,
    ) -> Atom {
        let arity = self.callee_arity(callee);
        if args.len() < arity {
            let wrapper = self.callee_wrapper(callee);
            return self.closure(wrapper, args, ty, out);
        }
        let rest = args.split_off(arity);
        let (name, rhs) = self.saturated_rhs(callee, args);
        if rest.is_empty() {
            return self.bind(out, name, ty, rhs);
        }
        let (_, function_ty) = split_arrows(callee_ty, arity);
        let function = self.bind(out, name, &function_ty, rhs);
        self.bind(out, "t", ty, Rhs::call(Call::Apply(function, rest)))
    }

    /// 本体が動き出すまでに受け取る引数の数。intrinsic と操作はシグネチャの外側の矢印の数、コンストラクタはフィールドの
    /// 数である。
    fn callee_arity(&self, callee: Callee) -> usize {
        match callee {
            Callee::Function(target) => self.program.arity(target),
            Callee::Intrinsic { function, .. } => self
                .hir
                .arity(function)
                .expect("an intrinsic has a signature"),
            Callee::Operation(op) => self.hir[op].arity,
            Callee::Constructor(ctor) => self.hir[ctor].fields.len(),
        }
    }

    /// 引数が足りないときに、クロージャにする関数。
    fn callee_wrapper(&mut self, callee: Callee) -> FnIdx {
        match callee {
            Callee::Function(target) => target,
            Callee::Intrinsic { function, .. } => self.program.wrapper(self.hir, function),
            Callee::Operation(op) => self.program.operation_wrapper(self.hir, op),
            Callee::Constructor(ctor) => self.program.constructor_wrapper(self.hir, ctor),
        }
    }

    /// 引数がちょうどそろったときの命令と、その結果を束縛する変数の名前。
    fn saturated_rhs(&self, callee: Callee, args: Vec<Atom>) -> (&'static str, Rhs) {
        match callee {
            Callee::Function(target) => ("t", Rhs::call(Call::Direct(target, args))),
            Callee::Intrinsic { function, callee } => {
                let lowering = intrinsic(&self.hir[function].name)
                    .expect("every intrinsic reaching Core IR has an implementation");
                let rhs = match lowering {
                    Lowering::Prim(op) => Rhs::Prim(op, args),
                    Lowering::Equality { negated } => {
                        let equality =
                            self.types.equalities.get(callee).copied().expect(
                                "the type checker decides how every `==` and `!=` compares",
                            );
                        Rhs::Prim(equality_op(equality, negated), args)
                    }
                };
                ("t", rhs)
            }
            Callee::Operation(op) => ("t", operation_rhs(self.hir, op, args)),
            Callee::Constructor(ctor) => (
                "d",
                Rhs::Con {
                    tag: self.hir[ctor].tag,
                    args,
                },
            ),
        }
    }

    /// `call_steps` の手順どおりに評価し、続けて並ぶ矢印を1回の呼び出しにする (docs/spec/expressions.md の「関数適用」)。
    /// 最初のまとまりは、呼ばれる式が既知なら呼ぶ相手の引数の数で場合分けし、それ以外は前の値への `Apply` にする。
    fn call(&mut self, id: ExprId, callee: ExprId, ty: &Type, out: &mut Bindings) -> Atom {
        let body = self.body;
        let ExprKind::Call { args, .. } = &body.exprs[id].kind else {
            unreachable!("call takes a call");
        };
        let callee_ty = self.ty(callee);
        let steps = eml_hir::call_steps(self.hir, body, id);
        let mut atoms: Vec<Option<Atom>> = vec![None; args.len()];
        // 前のまとまりの結果か、評価した呼ばれる式。既知の呼ばれる式は評価せず、最初のまとまりで直接呼ぶ
        let mut function: Option<Atom> = None;
        let mut applied = 0;
        let mut index = 0;
        while index < steps.len() {
            match steps[index] {
                EvalStep::Eval(expr) if expr == callee => {
                    if eml_hir::known_arity(self.hir, body, callee).is_none() {
                        function = Some(self.atom(callee, out));
                    }
                    index += 1;
                }
                EvalStep::Eval(expr) => {
                    let position = args
                        .iter()
                        .position(|&arg| arg == expr)
                        .expect("an evaluated part is the callee or an argument");
                    atoms[position] = Some(self.atom(expr, out));
                    index += 1;
                }
                EvalStep::Arrow(_) => {
                    let mut group = Vec::new();
                    while let Some(&EvalStep::Arrow(arg)) = steps.get(index) {
                        group.push(
                            atoms[arg]
                                .take()
                                .expect("an argument is evaluated before its arrow"),
                        );
                        index += 1;
                    }
                    applied += group.len();
                    let group_ty = if applied == args.len() {
                        ty.clone()
                    } else {
                        split_arrows(&callee_ty, applied).1
                    };
                    let result = match function {
                        None => self.call_head(callee, &callee_ty, group, &group_ty, out),
                        Some(value) => {
                            self.bind(out, "t", &group_ty, Rhs::call(Call::Apply(value, group)))
                        }
                    };
                    function = Some(result);
                }
            }
        }
        function.expect("a call has at least one argument")
    }

    /// 既知の呼ばれる式 (`eml_hir::known_arity` が `Some`) を、最初のまとまりの引数で呼ぶ。
    fn call_head(
        &mut self,
        callee: ExprId,
        callee_ty: &Type,
        args: Vec<Atom>,
        ty: &Type,
        out: &mut Bindings,
    ) -> Atom {
        let head = match &self.body.exprs[callee].kind {
            ExprKind::Path(Res::Function(function)) if self.hir[*function].intrinsic => {
                Callee::Intrinsic {
                    function: *function,
                    callee,
                }
            }
            ExprKind::Path(Res::Function(function)) => Callee::Function(self.indices[*function]),
            ExprKind::Path(Res::Operation(op)) => Callee::Operation(*op),
            ExprKind::Path(Res::Constructor(ctor)) => Callee::Constructor(*ctor),
            _ => unreachable!("only a known callee is called without evaluating it"),
        };
        self.saturate(head, callee_ty, args, ty, out)
    }

    /// 式の値をアトムにする。値の計算に要る束縛は `out` に積む。
    pub(super) fn atom(&mut self, id: ExprId, out: &mut Bindings) -> Atom {
        let body = self.body;
        match &body.exprs[id].kind {
            ExprKind::Missing => {
                unreachable!("a program without errors has no missing expressions")
            }
            ExprKind::Literal(Literal::Int(n)) => Atom::Int(*n),
            ExprKind::Literal(Literal::Unit) => Atom::Unit,
            ExprKind::Literal(Literal::String(text)) => {
                let index = self.program.strings.intern(text);
                let ty = self.lang_type(self.hir.lang.string);
                self.bind(out, "s", &ty, Rhs::ConstString(index))
            }
            ExprKind::Path(Res::Local(local)) => self.locals[*local],
            ExprKind::Path(Res::Function(function)) if self.hir[*function].intrinsic => {
                let wrapper = self.program.wrapper(self.hir, *function);
                let ty = self.ty(id);
                self.closure(wrapper, Vec::new(), &ty, out)
            }
            // 引数のないトップレベルの値は、参照するたびに呼び出す (docs/spec/core-ir.md)
            ExprKind::Path(Res::Function(function)) => {
                let target = self.indices[*function];
                if self.program.arity(target) == 0 {
                    let ty = self.ty(id);
                    let name = self.hir[*function].name.clone();
                    self.bind(out, &name, &ty, Rhs::call(Call::Direct(target, Vec::new())))
                } else {
                    let ty = self.ty(id);
                    self.closure(target, Vec::new(), &ty, out)
                }
            }
            ExprKind::Path(Res::Operation(op)) => {
                let wrapper = self.program.operation_wrapper(self.hir, *op);
                let ty = self.ty(id);
                self.closure(wrapper, Vec::new(), &ty, out)
            }
            ExprKind::Path(Res::Constructor(ctor)) => {
                let constructor = &self.hir[*ctor];
                if constructor.fields.is_empty() {
                    Atom::Tag(constructor.tag)
                } else {
                    let wrapper = self.program.constructor_wrapper(self.hir, *ctor);
                    let ty = self.ty(id);
                    self.closure(wrapper, Vec::new(), &ty, out)
                }
            }
            ExprKind::Call { callee, .. } => {
                let ty = self.ty(id);
                self.call(id, *callee, &ty, out)
            }
            ExprKind::If { .. } | ExprKind::Match { .. } => {
                // 続きの式を join point の本体にし、`if` と `match` の値をその引数で受ける。条件と scrutinee の計算も範囲に
                // 入れる。条件が末尾にない `if` のとき、その join point が外側の join point の範囲の中にでき、枝から外側へ
                // jump できる (docs/spec/core-ir.md)
                let join = self.new_join();
                let scope = self.tail(id, Exit::Jump(join));
                let ty = self.ty(id);
                let param = self.new_var("t", &ty);
                out.push(Binding::Join {
                    join,
                    params: vec![param],
                    scope,
                });
                Atom::Var(param)
            }
            ExprKind::Block { stmts, tail, .. } => {
                self.stmts(stmts, out);
                match tail {
                    Some(tail) => self.atom(*tail, out),
                    None => Atom::Unit,
                }
            }
            ExprKind::Annot { expr, .. } => self.atom(*expr, out),
            // 本体と節を、捕まえた変数を先頭の引数に持つ関数に持ち上げる (docs/spec/core-ir.md)。本体は `()` を受ける
            ExprKind::Handle {
                body: handled,
                init,
                effect,
                clauses,
                ret,
            } => {
                let effect = effect.expect("a program without errors handles a known effect");
                let ty = self.ty(id);
                // 初期値は本体と節を持ち上げる前に評価する (docs/spec/expressions.md のパラメータ付き handler)
                let init_atom = match init {
                    Some(init) => self.atom(*init, out),
                    None => Atom::Unit,
                };
                let prefix = format!("{}$handle{}", self.root_name, *self.handlers);
                *self.handlers += 1;
                let unit = [(None, Type::unit())];
                let handled_closure = self.lift(
                    prefix.clone(),
                    body.closure_captures(handled),
                    &unit,
                    handled.body,
                    &Type::Flexible,
                    out,
                );
                // 状態のある handler は HIR の節の引数が状態を含む。状態のない handler だけ、
                // 状態を `()` にして最後の引数で受ける (docs/spec/core-ir.md)
                let stateless = init.is_none();
                // 節はエフェクトの操作の順に並べる。インタプリタは操作の番号で節を引く
                let operations = self.hir[effect].operations.clone();
                let mut closures = Vec::new();
                for op in operations {
                    let clause = clauses
                        .iter()
                        .find(|clause| clause.op == op)
                        .expect("a program without errors has a clause for every operation");
                    let mut params: Vec<(Option<PatId>, Type)> = clause
                        .closure
                        .params
                        .iter()
                        .map(|&pat| (Some(pat), self.pat_type(pat)))
                        .collect();
                    if stateless {
                        params.push((None, Type::unit()));
                    }
                    let name = format!("{prefix}${}", self.hir[op].name);
                    let captured = body.closure_captures(&clause.closure);
                    let closure = self.lift(
                        name,
                        captured,
                        &params,
                        clause.closure.body,
                        &Type::Flexible,
                        out,
                    );
                    closures.push(closure);
                }
                let mut params: Vec<(Option<PatId>, Type)> = ret
                    .closure
                    .params
                    .iter()
                    .map(|&pat| (Some(pat), self.pat_type(pat)))
                    .collect();
                if stateless {
                    params.push((None, Type::unit()));
                }
                let captured = body.closure_captures(&ret.closure);
                let name = format!("{prefix}$return");
                let ret = self.lift(
                    name,
                    captured,
                    &params,
                    ret.closure.body,
                    &Type::Flexible,
                    out,
                );
                let call = Call::Handle {
                    effect: effect_index(self.hir, effect),
                    init: init_atom,
                    body: handled_closure,
                    clauses: closures,
                    ret,
                };
                self.bind(out, "t", &ty, Rhs::call(call))
            }
            ExprKind::Resume {
                k,
                arg,
                arg_end: _,
                state,
            } => {
                let k = self.atom(*k, out);
                let arg = self.atom(*arg, out);
                let state = match state {
                    Some(state) => self.atom(*state, out),
                    None => Atom::Unit,
                };
                let ty = self.ty(id);
                self.bind(out, "t", &ty, Rhs::call(Call::Resume { k, arg, state }))
            }
            ExprKind::Tuple(elements) => {
                // 要素を左から評価し、コンストラクタが1つの `data` と同じ値にする (docs/spec/core-ir.md)
                let args = elements
                    .iter()
                    .map(|&element| self.atom(element, out))
                    .collect();
                let ty = self.ty(id);
                self.bind(out, "d", &ty, Rhs::Con { tag: TUPLE, args })
            }
            ExprKind::Drop(value) => {
                let value = self.atom(*value, out);
                self.bind(out, "t", &Type::unit(), Rhs::Drop(value))
            }
            ExprKind::Lambda(closure) => {
                let Closure {
                    params,
                    body: lambda_body,
                } = closure;
                let lambda_ty = self.ty(id);
                let (param_types, _) = split_arrows(&lambda_ty, params.len());
                let params: Vec<(Option<PatId>, Type)> = params
                    .iter()
                    .map(|&pat| Some(pat))
                    .zip(param_types)
                    .collect();
                let name = format!("{}$lambda{}", self.root_name, *self.lambdas);
                *self.lambdas += 1;
                let captured = body.closure_captures(closure);
                self.lift(name, captured, &params, *lambda_body, &lambda_ty, out)
            }
        }
    }

    /// `_` と `()` で受けた値は以後使われないので、Perceus の挿入が decref する。
    pub(super) fn bind_pat(&mut self, pat: PatId, value: Atom) {
        for local in self.body.pat_bindings(pat) {
            self.locals.insert(local, value);
        }
    }
}
