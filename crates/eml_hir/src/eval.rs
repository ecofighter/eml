//! 呼び出しの評価の手順と、引数をまとめて渡す範囲 (docs/spec/expressions.md の「関数適用」)。持ち越しのパス (`eml_types`) と
//! Core IR の変換 (`eml_core_ir`) の両方がここを読む。2か所で順を組み立てると、ずれたときに持ち越し規則が実行と食い違い、
//! 健全でなくなるため。

use crate::{Body, ExprId, ExprKind, Module, Res};

/// 呼び出しの評価の1歩。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalStep {
    /// 部分式を評価する。
    Eval(ExprId),
    /// それまでの値に、引数 `i` (0 から数える) を渡す矢印を適用する。続けて並ぶ `Arrow` は1回の呼び出しにまとめる。
    Arrow(usize),
}

/// 呼び出し (`ExprKind::Call`) の評価の手順。呼ばれる式を評価し、引数ごとに、評価してから矢印を適用する (ML 式)。
/// 観測できる順を変えない範囲で、引数をまとめて渡す。既知の呼ばれる式の引数の数までの引数は、本体が引数のそろうまで
/// 動かないのでまとめる。それを超える引数は、値なら前とまとめ、値でなければその前で矢印を適用する。
pub fn call_steps(module: &Module, body: &Body, call: ExprId) -> Vec<EvalStep> {
    let ExprKind::Call {
        callee,
        args,
        evaluate_first,
    } = &body.exprs[call].kind
    else {
        panic!("call_steps takes a call");
    };
    let mut steps = Vec::new();
    // `x |> f a` の `x` は、呼ばれる式とほかの引数より先に評価する (docs/spec/declarations.md の標準の演算子の表)
    if let Some(first) = *evaluate_first {
        steps.push(EvalStep::Eval(args[first]));
    }
    steps.push(EvalStep::Eval(*callee));
    let known = known_arity(module, body, *callee).unwrap_or(0);
    let mut pending = Vec::new();
    for (index, &arg) in args.iter().enumerate() {
        let evaluated = Some(index) == *evaluate_first;
        if !(index < known || evaluated || is_value(module, body, arg)) {
            steps.extend(pending.drain(..).map(EvalStep::Arrow));
        }
        if !evaluated {
            steps.push(EvalStep::Eval(arg));
        }
        pending.push(index);
    }
    steps.extend(pending.into_iter().map(EvalStep::Arrow));
    steps
}

/// 評価しても何も起きない式か。まとめて渡しても、観測できる順が変わらない。
pub fn is_value(module: &Module, body: &Body, expr: ExprId) -> bool {
    match &body.exprs[expr].kind {
        ExprKind::Literal(_) | ExprKind::Lambda(_) => true,
        ExprKind::Path(Res::Local(_) | Res::Operation(_) | Res::Constructor(_)) => true,
        // 引数のないトップレベルの値は、参照するたびに計算する (docs/spec/core-ir.md)
        ExprKind::Path(Res::Function(function)) => module.functions[*function]
            .arity()
            .is_some_and(|arity| arity > 0),
        ExprKind::Annot { expr, .. } => is_value(module, body, *expr),
        _ => false,
    }
}

/// 引数がそろうまで本体が動かない、呼ばれる式の引数の数。引数のないトップレベルの値は、参照するたびに計算して関数値を
/// 返すので含めない。`Some` の呼ばれる式は、クロージャを作らずに最初のまとまりで直接呼ぶ。`call_steps` はこの式の
/// `Eval` も並べるが、Core IR の変換は評価を飛ばす。Core IR の変換も、直接呼ぶかどうかをこれで決める。
pub fn known_arity(module: &Module, body: &Body, callee: ExprId) -> Option<usize> {
    match &body.exprs[callee].kind {
        ExprKind::Path(Res::Function(function)) => module.functions[*function]
            .arity()
            .filter(|&arity| arity > 0),
        ExprKind::Path(Res::Operation(op)) => Some(module.operations[*op].arity),
        ExprKind::Path(Res::Constructor(ctor)) => Some(module.constructors[*ctor].fields.len()),
        _ => None,
    }
}
