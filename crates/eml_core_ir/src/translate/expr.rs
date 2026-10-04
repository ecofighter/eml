//! 式ごとの変換と、呼び出しの引数の個数による場合分け (docs/spec/core-ir.md の eval/apply)。

use eml_hir::builtin::Builtin;
use eml_hir::{ExprId, ExprKind, Literal, OperationId, PatId, Res};
use eml_types::Type;

use crate::{Atom, Call, FnIdx, JoinId, Rhs};

use super::program::{effect_index, perform_call};
use super::types::{Lowering, lowering, split_arrows};
use super::{Binding, Bindings, Exit, FnLowering};

impl FnLowering<'_> {
    fn ty(&self, expr: ExprId) -> Type {
        self.types
            .exprs
            .get(expr)
            .cloned()
            .expect("every reached expression is typed")
    }

    pub(super) fn bind(&mut self, out: &mut Bindings, name: &str, ty: &Type, rhs: Rhs) -> Atom {
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
            Lowering::Io(op) => Rhs::Io(op, args),
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

    /// 引数が操作の引数の個数に揃えば `perform` にし、足りなければ操作を包む関数のクロージャにする。操作の引数の個数は
    /// シグネチャの外側の矢印の数なので、型検査を通った呼び出しで引数が余ることはない。
    fn call_operation(
        &mut self,
        op: OperationId,
        args: Vec<Atom>,
        ty: &Type,
        out: &mut Bindings,
    ) -> Atom {
        let arity = self.module.operations[op].arity;
        if args.len() < arity {
            let wrapper = self.program.operation_wrapper(self.module, op);
            return self.bind(out, "c", ty, Rhs::MakeClosure(wrapper, args));
        }
        let call = perform_call(self.module, op, args);
        self.bind(out, "t", ty, Rhs::call(call))
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
            ExprKind::Path(Res::Operation(op)) => {
                let wrapper = self.program.operation_wrapper(self.module, *op);
                let ty = self.ty(id);
                self.bind(out, "c", &ty, Rhs::MakeClosure(wrapper, Vec::new()))
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
                    ExprKind::Path(Res::Operation(op)) => {
                        let args = self.call_args(args, first, out);
                        self.call_operation(*op, args, &ty, out)
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
                // 続きの式を join point の本体にし、`if` の値をその引数で受ける。条件の計算も範囲に入れる。条件が末尾に
                // ない `if` のとき、その join point が外側の join point の範囲の中にでき、枝から外側へ jump できる
                // (docs/spec/core-ir.md)
                let join = JoinId(self.joins.len() as u32);
                self.joins.push(None);
                let scope = self.tail(id, Exit::Jump(join));
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
            // 本体と節を、捕まえた変数を先頭の引数に持つ関数に持ち上げる (docs/spec/core-ir.md)。本体は `()` を受ける
            ExprKind::Handle {
                body: handled,
                effect,
                clauses,
                ret,
            } => {
                let effect = effect.expect("a program without errors handles a known effect");
                let ty = self.ty(id);
                let prefix = format!("{}$handle{}", self.root_name, *self.handlers);
                *self.handlers += 1;
                let unit = [(None, Type::unit())];
                let handled_closure = self.lift(
                    prefix.clone(),
                    body.captures(*handled, &[]),
                    &unit,
                    *handled,
                    &Type::Flexible,
                    out,
                );
                // 節はエフェクトの操作の順に並べる。インタプリタは操作の番号で節を引く
                let operations = self.module.effects[effect].operations.clone();
                let mut closures = Vec::new();
                for op in operations {
                    let clause = clauses
                        .iter()
                        .find(|clause| clause.op == op)
                        .expect("a program without errors has a clause for every operation");
                    let bound: Vec<PatId> = clause.patterns().collect();
                    let params: Vec<(Option<PatId>, Type)> = bound
                        .iter()
                        .map(|&pat| (Some(pat), self.pat_type(pat)))
                        .collect();
                    let name = format!("{prefix}${}", self.module.operations[op].name);
                    let captured = body.captures(clause.body, &bound);
                    let closure =
                        self.lift(name, captured, &params, clause.body, &Type::Flexible, out);
                    closures.push(closure);
                }
                let ret = ret.as_ref().map(|ret| {
                    let params = [(Some(ret.param), self.pat_type(ret.param))];
                    let captured = body.captures(ret.body, &[ret.param]);
                    let name = format!("{prefix}$return");
                    self.lift(name, captured, &params, ret.body, &Type::Flexible, out)
                });
                let call = Call::Handle {
                    effect: effect_index(effect),
                    body: handled_closure,
                    clauses: closures,
                    ret,
                };
                self.bind(out, "t", &ty, Rhs::call(call))
            }
            ExprKind::Resume { k, arg } => {
                let k = self.atom(*k, out);
                let arg = self.atom(*arg, out);
                let ty = self.ty(id);
                self.bind(out, "t", &ty, Rhs::call(Call::Resume { k, arg }))
            }
            ExprKind::Drop(value) => {
                let value = self.atom(*value, out);
                self.bind(out, "t", &Type::unit(), Rhs::Drop(value))
            }
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                let lambda_ty = self.ty(id);
                let (param_types, _) = split_arrows(&lambda_ty, params.len());
                let params: Vec<(Option<PatId>, Type)> = params
                    .iter()
                    .map(|&pat| Some(pat))
                    .zip(param_types)
                    .collect();
                let name = format!("{}$lambda{}", self.root_name, *self.lambdas);
                *self.lambdas += 1;
                let captured = body.lambda_captures(id);
                self.lift(name, captured, &params, *lambda_body, &lambda_ty, out)
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
    pub(super) fn bind_pat(&mut self, pat: PatId, value: Atom) {
        for local in self.body.pat_bindings(pat) {
            self.locals.insert(local, value);
        }
    }
}
