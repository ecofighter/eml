//! 呼び出しの評価の手順と、直接呼ぶ範囲 (docs/spec/expressions.md の「関数適用」)。持ち越しのパス (`eml_types`) と
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

/// 呼び出し (`ExprKind::Call`) の評価の手順。呼ばれる式と引数をこの順にすべて評価してから、矢印を順に適用する
/// (OCaml の `f a b` と同じ順)。矢印の適用の間に評価は入らないので、引数をまとめて渡しても観測できる順は変わらない。
pub fn call_steps(body: &Body, call: ExprId) -> Vec<EvalStep> {
    let ExprKind::Call { callee, args } = &body.exprs[call].kind else {
        panic!("call_steps takes a call");
    };
    let mut steps = vec![EvalStep::Eval(*callee)];
    steps.extend(args.iter().map(|&arg| EvalStep::Eval(arg)));
    steps.extend((0..args.len()).map(EvalStep::Arrow));
    steps
}

/// 引数がそろうまで本体が動かない、呼ばれる式の引数の数。引数のないトップレベルの値は、参照するたびに計算して関数値を
/// 返すので含めない。`Some` の呼ばれる式は、クロージャを作らずに引数の数までの引数で直接呼び、余った引数は返った
/// 関数値に適用する。`call_steps` はこの式の `Eval` も並べるが、Core IR の変換は評価を飛ばす。持ち越し規則と変換が同じ
/// 手順で `k v st` を1回の再開にするよう、節の `k` もここで引数の数を答える。
pub fn known_arity(program: &Program, body: &Body, callee: ExprId) -> Option<usize> {
    match &body.exprs[callee].kind {
        ExprKind::Path(Res::Item(ValueItem::Function(function))) => {
            program.arity(*function).filter(|&arity| arity > 0)
        }
        // どの instance を選んでも直接呼ぶ範囲が同じになるよう、instance の等式の引数の数ではなく、メソッドの
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
