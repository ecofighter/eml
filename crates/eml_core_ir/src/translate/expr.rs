//! 式ごとの変換と、呼び出しの引数の個数による場合分け (docs/spec/core-ir.md の eval/apply)。

use eml_extern::{Extern, ExternType};
use eml_hir::EvalStep;
use eml_hir::{
    Closure, ConstructorId, ExprId, ExprKind, FunctionId, FunctionKind, Literal, OperationId,
    PatId, Program as HirProgram, Res, TypeDefId, ValueItem,
};
use eml_types::Type;

use crate::{Atom, Call, Ctor, FnIdx, Rhs, Stmt, TUPLE};

use super::pattern::Known;
use super::program::{effect_index, perform_call, plain_call};
use super::types::{equality_extern, split_arrows, var_info};
use super::{ContinuationForm, Exit, FnLowering};

/// extern の関数なら、その表の行。誤りのないプログラムの extern は、どれも標準ライブラリの宣言で行を持つ。
pub(super) fn extern_row(hir: &HirProgram, function: FunctionId) -> Option<Extern> {
    match hir[function].kind {
        FunctionKind::Extern(row) => {
            Some(row.expect("a program without errors has no user extern"))
        }
        FunctionKind::Defined => None,
    }
}

/// 既知の呼ばれる式の種類。`saturate` は、引数の数、足りないときの包む関数、ちょうどのときの命令を、この種類から決める
/// (docs/implementation/architecture.md の「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」)。
#[derive(Clone, Copy)]
enum Callee {
    /// 本体のある関数。足りないときの包む関数は、その関数自身である。
    Function(FnIdx),
    /// extern の関数。`callee` は呼ばれる式で、型で選ぶ行 (`==` と `!=`) の比べる値の型を
    /// `BodyTypes::instantiations` から引くのと、呼び出しの位置に使う。
    Extern {
        function: FunctionId,
        row: Extern,
        callee: ExprId,
    },
    Operation(OperationId),
    Constructor(ConstructorId),
    /// 直接の形の節の `k`。`k` は生の継続で、`arity` は状態のない handler で1、状態のある handler で2である。
    Continuation {
        k: Atom,
        arity: usize,
    },
}

/// `mask` 付きの呼び出し。`saved` は Perceus が埋める。
fn masked_call(call: Call, mask: Vec<u32>) -> Rhs {
    Rhs::Call {
        call,
        mask,
        saved: Vec::new(),
    }
}

impl FnLowering<'_> {
    pub(super) fn ty(&self, expr: ExprId) -> Type {
        self.ctx
            .types
            .exprs
            .get(expr)
            .cloned()
            .expect("every reached expression is typed")
    }

    /// 組み込みの型 (`String`、`Bool`) の、引数のない型構成子の型。
    fn lang_type(&self, id: TypeDefId) -> Type {
        Type::Con {
            id,
            args: Vec::new(),
        }
    }

    /// `rhs` の値を新しい変数に束縛する文を今のブロックに足す。`con` で作った変数は中身を覚え、後の決定木がその頭で
    /// case を選べるようにする (docs/spec/core-ir.md)。
    pub(super) fn bind(&mut self, name: &str, ty: &Type, rhs: Rhs) -> Atom {
        let var = self.builder.var(var_info(name, ty, self.ctx.hir));
        if let Rhs::Con { ctor, args } = &rhs {
            let known = Known {
                tag: ctor.tag,
                args: args.clone(),
                ty: ty.clone(),
            };
            self.cons.insert(var, known);
        }
        self.builder.emit(Stmt::Let { var, rhs });
        Atom::Var(var)
    }

    /// 値として使う extern の参照 `site` ごとの包む関数 (docs/spec/core-ir.md)。
    fn extern_wrapper(&mut self, site: ExprId, function: FunctionId, row: Extern) -> FnIdx {
        let number = self.ctx.numbering.externs[site];
        let name = format!("{}$extern{number}", self.ctx.root_name);
        let at = self.loc(site);
        self.program
            .extern_wrapper(self.ctx.hir, function, row, name, at)
    }

    /// 呼ぶ相手の引数の個数と比べ、揃えば命令にし、足りなければ包む関数のクロージャにし、余れば命令の結果に残りを
    /// 適用する (docs/spec/core-ir.md の eval/apply)。呼ばれる式の種類によらず、この1か所で場合分けする。
    /// `id` は呼び出しの式で、`args` は矢印 0 から渡す。既知の呼ばれる式は最初のまとまりで呼ぶためである。
    fn saturate(
        &mut self,
        id: ExprId,
        callee: Callee,
        callee_ty: &Type,
        mut args: Vec<Atom>,
        ty: &Type,
    ) -> Atom {
        let arity = self.callee_arity(callee);
        // 部分適用はクロージャを作るだけでエフェクトを起こさないので、`mask` を付けない
        if args.len() < arity {
            let wrapper = self.callee_wrapper(callee);
            return self.closure(wrapper, args, ty);
        }
        let rest = args.split_off(arity);
        let (name, rhs) = self.saturated_rhs(id, callee, args);
        if rest.is_empty() {
            return self.bind(name, ty, rhs);
        }
        let (_, function_ty) = split_arrows(callee_ty, arity);
        let function = self.bind(name, &function_ty, rhs);
        self.apply(id, callee_ty, function, arity, rest, ty)
    }

    /// `function` に、呼び出し `id` の矢印 `first` からの引数 `args` を渡す。`ty` は最後の `apply` の結果の型である。
    /// `mask` は1回の Core IR の呼び出し全体に効くので、矢印ごとの `mask` が変わる境目で `apply` を分ける。違う `mask`
    /// の矢印を1つにまとめると、片方の矢印に余計な `mask` が効くためである (docs/spec/core-ir.md)。
    fn apply(
        &mut self,
        id: ExprId,
        callee_ty: &Type,
        mut function: Atom,
        first: usize,
        args: Vec<Atom>,
        ty: &Type,
    ) -> Atom {
        let last = first + args.len();
        let masks: Vec<Vec<u32>> = (first..last).map(|arrow| self.mask(id, arrow)).collect();
        let mut args = args.into_iter();
        let mut end = first;
        for run in masks.chunk_by(|a, b| a == b) {
            end += run.len();
            let part = args.by_ref().take(run.len()).collect();
            let part_ty = if end == last {
                ty.clone()
            } else {
                split_arrows(callee_ty, end).1
            };
            let rhs = masked_call(Call::Apply(function, part), run[0].clone());
            function = self.bind("t", &part_ty, rhs);
        }
        function
    }

    /// 型検査が記録した、呼び出し `call` の矢印 `arrow` の `mask` を、エフェクトの番号の昇順で返す (docs/spec/core-ir.md)。
    fn mask(&self, call: ExprId, arrow: usize) -> Vec<u32> {
        let mut mask: Vec<u32> = self
            .ctx
            .types
            .masks
            .get(&(call, arrow))
            .map(|effects| {
                effects
                    .iter()
                    .map(|&effect| effect_index(self.ctx.hir, effect))
                    .collect()
            })
            .unwrap_or_default();
        mask.sort_unstable();
        mask
    }

    /// 本体が動き出すまでに受け取る引数の数。extern の関数は表の行の引数の数、操作はシグネチャの外側の矢印の数、
    /// コンストラクタはフィールドの数である。
    fn callee_arity(&self, callee: Callee) -> usize {
        match callee {
            Callee::Function(target) => self.program.arity(target),
            Callee::Extern {
                function: _,
                row,
                callee: _,
            } => row.row().params.len(),
            Callee::Operation(op) => self.ctx.hir[op].arity,
            Callee::Constructor(ctor) => self.ctx.hir[ctor].fields.len(),
            Callee::Continuation { k: _, arity } => arity,
        }
    }

    /// 引数が足りないときに、クロージャにする関数。
    fn callee_wrapper(&mut self, callee: Callee) -> FnIdx {
        match callee {
            Callee::Function(target) => target,
            Callee::Extern {
                function,
                row,
                callee,
            } => self.extern_wrapper(callee, function, row),
            Callee::Operation(op) => self.program.operation_wrapper(self.ctx.hir, op),
            Callee::Constructor(ctor) => self.program.constructor_wrapper(self.ctx.hir, ctor),
            // 引数の足りない `k` の呼び出しは `k` を関数の値として使うことになり、節を包む形にする (`continuation_forms`)
            Callee::Continuation { k: _, arity: _ } => {
                unreachable!("a continuation in the direct form is always saturated")
            }
        }
    }

    /// 引数がちょうどそろったときの命令と、その結果を束縛する変数の名前。
    /// `mask` を付けるのは本体のある関数だけである。extern の関数、操作、コンストラクタは、型検査が row を開かずに
    /// 宣言のまま含めるので、`mask` が記録されない。
    fn saturated_rhs(
        &mut self,
        id: ExprId,
        callee: Callee,
        args: Vec<Atom>,
    ) -> (&'static str, Rhs) {
        // `Extern`、`Perform`、`Con` は `mask` を持てない (docs/spec/core-ir.md)。型検査が `mask` を記録するように
        // 変わると、ここで気づかないうちに落とすことになる
        debug_assert!(
            matches!(callee, Callee::Function(_) | Callee::Continuation { .. })
                || (0..args.len()).all(|arrow| self.mask(id, arrow).is_empty()),
            "an extern, operation or constructor call has no recorded mask"
        );
        match callee {
            // 前の矢印は部分適用でエフェクトを起こさないので、最後の矢印の `mask` だけを使う (docs/spec/core-ir.md)
            Callee::Function(target) => {
                let mask = self.mask(id, args.len() - 1);
                ("t", masked_call(Call::Direct(target, args), mask))
            }
            Callee::Extern {
                function: _,
                row,
                callee,
            } => {
                let ext = if row.row().by_type {
                    let instantiation = self
                        .ctx
                        .types
                        .instantiations
                        .get(callee)
                        .expect("the type checker records every reference to `==` and `!=`");
                    let equality = eml_types::equality(self.ctx.hir, &instantiation.args[0])
                        .expect("the type checker reports every `==` and `!=` it cannot decide");
                    equality_extern(equality, row == Extern::Ne)
                } else {
                    row
                };
                let at = Some(self.loc(callee));
                ("t", Rhs::Extern { ext, args, at })
            }
            Callee::Operation(op) => ("t", plain_call(perform_call(self.ctx.hir, op, args))),
            // 状態ありの最初の矢印は row が空の部分適用なので、関数と同じく最後の矢印の `mask` を使う
            Callee::Continuation { k, arity: _ } => {
                let mask = self.mask(id, args.len() - 1);
                let mut args = args.into_iter();
                let arg = args.next().expect("a continuation takes a value");
                let state = args.next().unwrap_or(Atom::Unit);
                ("t", masked_call(Call::Resume { k, arg, state }, mask))
            }
            Callee::Constructor(ctor) => (
                "d",
                Rhs::Con {
                    ctor: self.program.ctor(self.ctx.hir, ctor),
                    args,
                },
            ),
        }
    }

    /// `call_steps` の手順どおりに評価し、続けて並ぶ矢印を1回の呼び出しにする (docs/spec/expressions.md の「関数適用」)。
    /// 最初のまとまりは、呼ばれる式が既知なら呼ぶ相手の引数の数で場合分けし、それ以外は前の値への `Apply` にする。
    fn call(&mut self, id: ExprId, callee: ExprId, ty: &Type) -> Atom {
        let body = self.ctx.body;
        let ExprKind::Call { args, .. } = &body.exprs[id].kind else {
            unreachable!("call takes a call");
        };
        let callee_ty = self.ty(callee);
        let steps = eml_hir::call_steps(self.ctx.hir, body, id);
        let mut atoms: Vec<Option<Atom>> = vec![None; args.len()];
        // 前のまとまりの結果か、評価した呼ばれる式。既知の呼ばれる式は評価せず、最初のまとまりで直接呼ぶ
        let mut function: Option<Atom> = None;
        let mut applied = 0;
        let mut index = 0;
        while index < steps.len() {
            match steps[index] {
                EvalStep::Eval(expr) if expr == callee => {
                    if eml_hir::known_arity(self.ctx.hir, body, callee).is_none() {
                        function = Some(self.atom(callee));
                    }
                    index += 1;
                }
                EvalStep::Eval(expr) => {
                    let position = args
                        .iter()
                        .position(|&arg| arg == expr)
                        .expect("an evaluated part is the callee or an argument");
                    atoms[position] = Some(self.atom(expr));
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
                    let first = applied;
                    applied += group.len();
                    let group_ty = if applied == args.len() {
                        ty.clone()
                    } else {
                        split_arrows(&callee_ty, applied).1
                    };
                    let result = match function {
                        None => self.call_head(id, callee, &callee_ty, group, &group_ty),
                        Some(value) => self.apply(id, &callee_ty, value, first, group, &group_ty),
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
        id: ExprId,
        callee: ExprId,
        callee_ty: &Type,
        args: Vec<Atom>,
        ty: &Type,
    ) -> Atom {
        let head = match &self.ctx.body.exprs[callee].kind {
            ExprKind::Path(Res::Item(ValueItem::Function(function))) => {
                match extern_row(self.ctx.hir, *function) {
                    Some(row) => Callee::Extern {
                        function: *function,
                        row,
                        callee,
                    },
                    None => Callee::Function(self.ctx.indices[*function]),
                }
            }
            ExprKind::Path(Res::Item(ValueItem::Operation(op))) => Callee::Operation(*op),
            ExprKind::Path(Res::Item(ValueItem::Constructor(ctor))) => Callee::Constructor(*ctor),
            ExprKind::Path(Res::Local(local)) => {
                let k = self.locals[*local];
                match self.ctx.continuation_forms[*local] {
                    ContinuationForm::Direct => Callee::Continuation {
                        k,
                        arity: self.ctx.body.continuations[*local],
                    },
                    // 包んだ `k` はクロージャなので、ほかの関数値と同じく矢印ごとの `mask` で `apply` する
                    ContinuationForm::Wrapped => {
                        return self.apply(id, callee_ty, k, 0, args, ty);
                    }
                }
            }
            _ => unreachable!("only a known callee is called without evaluating it"),
        };
        self.saturate(id, head, callee_ty, args, ty)
    }

    /// 値の `if` と `match` の値。続きをラベルにして、枝からそこへ向かう。続きに向かうブロックが1本なら、そのブロック
    /// で続きを変換する。
    fn through_continuation(&mut self, id: ExprId) -> Atom {
        let ty = self.ty(id);
        let after = self
            .builder
            .new_label(vec![var_info("t", &ty, self.ctx.hir)]);
        self.tail_expr(id, Exit::Jump(after));
        let args = self
            .builder
            .resolve(after)
            .expect("some arm of a value `if` or `match` reaches its continuation");
        let [value] = args[..] else {
            unreachable!("the continuation takes the value");
        };
        value
    }

    /// 式の値をアトムにする。値の計算に要る文は今のブロックに足す。
    pub(super) fn atom(&mut self, id: ExprId) -> Atom {
        let body = self.ctx.body;
        match &body.exprs[id].kind {
            ExprKind::Missing => {
                unreachable!("a program without errors has no missing expressions")
            }
            ExprKind::Literal(Literal::Int(n)) => Atom::Int(*n),
            ExprKind::Literal(Literal::Unit) => Atom::Unit,
            ExprKind::Literal(Literal::String(text)) => {
                let index = self.program.strings.intern(text);
                let ty = self.lang_type(self.ctx.hir.extern_type(ExternType::String));
                self.bind("s", &ty, Rhs::ConstString(index))
            }
            ExprKind::Path(Res::Local(local)) => self.locals[*local],
            ExprKind::Path(Res::Item(ValueItem::Function(function))) => {
                if let Some(row) = extern_row(self.ctx.hir, *function) {
                    let wrapper = self.extern_wrapper(id, *function, row);
                    let ty = self.ty(id);
                    return self.closure(wrapper, Vec::new(), &ty);
                }
                // 引数のないトップレベルの値は、参照するたびに呼び出す (docs/spec/core-ir.md)
                let target = self.ctx.indices[*function];
                let ty = self.ty(id);
                if self.program.arity(target) == 0 {
                    let name = self.ctx.hir[*function].name.clone();
                    self.bind(&name, &ty, plain_call(Call::Direct(target, Vec::new())))
                } else {
                    self.closure(target, Vec::new(), &ty)
                }
            }
            ExprKind::Path(Res::Item(ValueItem::Operation(op))) => {
                let wrapper = self.program.operation_wrapper(self.ctx.hir, *op);
                let ty = self.ty(id);
                self.closure(wrapper, Vec::new(), &ty)
            }
            ExprKind::Path(Res::Item(ValueItem::Constructor(ctor))) => {
                let constructor = &self.ctx.hir[*ctor];
                if constructor.fields.is_empty() {
                    Atom::Tag(constructor.tag)
                } else {
                    let wrapper = self.program.constructor_wrapper(self.ctx.hir, *ctor);
                    let ty = self.ty(id);
                    self.closure(wrapper, Vec::new(), &ty)
                }
            }
            ExprKind::Call { callee, .. } => {
                let ty = self.ty(id);
                self.call(id, *callee, &ty)
            }
            ExprKind::If { .. } | ExprKind::Match { .. } => self.through_continuation(id),
            // `let x = S; match x` は `S` を `match` の文脈で変換するので、`match` と同じく続きで値を受ける
            ExprKind::Block { stmts, tail, .. } if self.fused_match(stmts, *tail).is_some() => {
                self.through_continuation(id)
            }
            ExprKind::Block { stmts, tail, .. } => {
                self.stmts(stmts);
                match tail {
                    Some(tail) => self.atom(*tail),
                    None => Atom::Unit,
                }
            }
            ExprKind::Annot { expr, .. } => self.atom(*expr),
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
                    Some(init) => self.atom(*init),
                    None => Atom::Unit,
                };
                let prefix = format!(
                    "{}$handle{}",
                    self.ctx.root_name, self.ctx.numbering.handlers[id]
                );
                let unit = [(None, Type::unit())];
                let handled_ty = self.ty(handled.body);
                let handled_closure = self.lift(
                    prefix.clone(),
                    body.closure_captures(handled),
                    &unit,
                    handled.body,
                    &handled_ty,
                    &Type::Flexible,
                );
                // 状態のある handler は HIR の節の引数が状態を含む。状態のない handler だけ、
                // 状態を `()` にして最後の引数で受ける (docs/spec/core-ir.md)
                let stateless = init.is_none();
                // 節はエフェクトの操作の順に並べる。インタプリタは操作の番号で節を引く
                let operations = self.ctx.hir[effect].operations.clone();
                let mut closures = Vec::new();
                for op in operations {
                    let clause = clauses
                        .iter()
                        .find(|clause| clause.op == op)
                        .expect("a program without errors has a clause for every operation");
                    let name = format!("{prefix}${}", self.ctx.hir[op].name);
                    closures.push(self.lift_clause(name, &clause.closure, stateless));
                }
                let ret = self.lift_clause(format!("{prefix}$return"), &ret.closure, stateless);
                let call = Call::Handle {
                    effect: effect_index(self.ctx.hir, effect),
                    init: init_atom,
                    body: handled_closure,
                    clauses: closures,
                    ret,
                };
                self.bind("t", &ty, plain_call(call))
            }
            ExprKind::Tuple(elements) => {
                // 要素を左から評価し、コンストラクタが1つの `data` と同じ値にする (docs/spec/core-ir.md)
                let args: Vec<Atom> = elements.iter().map(|&element| self.atom(element)).collect();
                let ty = self.ty(id);
                let ctor = Ctor {
                    layout: self.program.tuple_layout(args.len()),
                    tag: TUPLE,
                };
                self.bind("d", &ty, Rhs::Con { ctor, args })
            }
            ExprKind::Drop(value) => {
                let value = self.atom(*value);
                self.bind("t", &Type::unit(), Rhs::Drop(value))
            }
            ExprKind::Lambda(closure) => {
                let Closure {
                    params,
                    body: lambda_body,
                } = closure;
                let lambda_ty = self.ty(id);
                let (param_types, ret_ty) = split_arrows(&lambda_ty, params.len());
                let params: Vec<(Option<PatId>, Type)> = params
                    .iter()
                    .map(|&pat| Some(pat))
                    .zip(param_types)
                    .collect();
                let name = format!(
                    "{}$lambda{}",
                    self.ctx.root_name, self.ctx.numbering.lambdas[id]
                );
                let captured = body.closure_captures(closure);
                self.lift(name, captured, &params, *lambda_body, &ret_ty, &lambda_ty)
            }
        }
    }

    /// handler の節か `return` の節を持ち上げる。状態のない handler の節は、最後の引数で状態の `()` を受ける
    /// (docs/spec/core-ir.md)。
    fn lift_clause(&mut self, name: String, closure: &Closure, stateless: bool) -> Atom {
        let mut params: Vec<(Option<PatId>, Type)> = closure
            .params
            .iter()
            .map(|&pat| (Some(pat), self.pat_type(pat)))
            .collect();
        if stateless {
            params.push((None, Type::unit()));
        }
        let captured = self.ctx.body.closure_captures(closure);
        let ty = self.ty(closure.body);
        self.lift(name, captured, &params, closure.body, &ty, &Type::Flexible)
    }
}
