//! 呼び出しの評価の手順と、引数をまとめて渡す範囲 (docs/spec/expressions.md の「関数適用」)。持ち越しのパス (`eml_types`) と
//! Core IR の変換 (`eml_core_ir`) の両方がここを読む。2か所で順を組み立てると、ずれたときに持ち越し規則が実行と食い違い、
//! 健全でなくなるため。

use crate::{Body, ExprId, ExprKind, Program, Res, ValueItem};

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
pub fn call_steps(program: &Program, body: &Body, call: ExprId) -> Vec<EvalStep> {
    let ExprKind::Call { callee, args } = &body.exprs[call].kind else {
        panic!("call_steps takes a call");
    };
    let mut steps = vec![EvalStep::Eval(*callee)];
    let known = known_arity(program, body, *callee).unwrap_or(0);
    let mut pending = Vec::new();
    for (index, &arg) in args.iter().enumerate() {
        if !(index < known || is_value(program, body, arg)) {
            steps.extend(pending.drain(..).map(EvalStep::Arrow));
        }
        steps.push(EvalStep::Eval(arg));
        pending.push(index);
    }
    steps.extend(pending.into_iter().map(EvalStep::Arrow));
    steps
}

/// 評価しても何も起きない式か。まとめて渡しても、観測できる順が変わらない。
pub fn is_value(program: &Program, body: &Body, expr: ExprId) -> bool {
    match &body.exprs[expr].kind {
        ExprKind::Literal(_) | ExprKind::Lambda(_) => true,
        ExprKind::Path(
            Res::Local(_) | Res::Item(ValueItem::Operation(_) | ValueItem::Constructor(_)),
        ) => true,
        // 引数のないトップレベルの値は、参照するたびに計算する (docs/spec/core-ir.md)
        ExprKind::Path(Res::Item(ValueItem::Function(function))) => {
            program.arity(*function).is_some_and(|arity| arity > 0)
        }
        ExprKind::Path(Res::Item(ValueItem::Method(method))) => {
            program[*method].signature.arity() > 0
        }
        ExprKind::Annot { expr, .. } => is_value(program, body, *expr),
        _ => false,
    }
}

/// 引数がそろうまで本体が動かない、呼ばれる式の引数の数。引数のないトップレベルの値は、参照するたびに計算して関数値を
/// 返すので含めない。`Some` の呼ばれる式は、クロージャを作らずに最初のまとまりで直接呼ぶ。`call_steps` はこの式の
/// `Eval` も並べるが、Core IR の変換は評価を飛ばす。Core IR の変換も、直接呼ぶかどうかをこれで決める。持ち越し規則と
/// 変換が同じ手順で `k v st` を1回の再開にするよう、節の `k` もここで引数の数を答える。
pub fn known_arity(program: &Program, body: &Body, callee: ExprId) -> Option<usize> {
    match &body.exprs[callee].kind {
        ExprKind::Path(Res::Item(ValueItem::Function(function))) => {
            program.arity(*function).filter(|&arity| arity > 0)
        }
        // どの instance を選んでも評価の順が同じになるよう、instance の等式の引数の数ではなく、メソッドの
        // シグネチャの矢印の数で決める
        ExprKind::Path(Res::Item(ValueItem::Method(method))) => {
            Some(program[*method].signature.arity()).filter(|&arity| arity > 0)
        }
        ExprKind::Path(Res::Item(ValueItem::Operation(op))) => Some(program[*op].arity),
        ExprKind::Path(Res::Item(ValueItem::Constructor(ctor))) => {
            Some(program[*ctor].fields.len())
        }
        ExprKind::Path(Res::Local(local)) => body.continuations.get(*local).copied(),
        _ => None,
    }
}
