//! 戻らない呼び出し (docs/superpowers/specs/2026-10-17-s13c-unwinding-design.md の「戻らない呼び出し」)。結果の型が
//! ほかのどこにも現れない量化された型変数なら、パラメトリシティからその型の値を作れないので、呼び出しは普通には戻らない。
//! 処理系は、これを実行時に確かめない。

use eml_hir::{
    Body, ExprId, ExprKind, Program, Res, RowRef, Signature, TypeRefId, TypeRefKind, TypeVarId,
    ValueItem,
};

/// `call` が戻らない呼び出しか。呼ばれるものは型スキームを持つ宣言への参照でなければならない。ローカル関数、ラムダの
/// 引数、`let` で束縛した値は単相で、結果の型が呼び出しごとに決まるので当たらない。
pub(crate) fn diverging_call(program: &Program, body: &Body, call: ExprId) -> bool {
    let ExprKind::Call { callee, args } = &body.exprs[call].kind else {
        return false;
    };
    let ExprKind::Path(Res::Item(item)) = body.exprs[*callee].kind else {
        return false;
    };
    // 先頭の `fixed` 個の型変数は、宣言ごとに量化した変数ではないか、クラスの制約が付いている。操作ではエフェクトの
    // 型引数で、参照の row に現れる。メソッドではクラスの型変数で、参照ごとに制約 `C a` が付く
    let (signature, fixed) = match item {
        ValueItem::Function(function) => match &program[function].signature {
            Some(signature) => (signature, 0),
            None => return false,
        },
        ValueItem::Operation(op) => (&program[op].signature, program[op].effect_params),
        ValueItem::Method(method) => (&program[method].signature, 1),
        ValueItem::Constructor(_) | ValueItem::Binding(_) => return false,
    };
    if args.len() < signature.arity() {
        return false;
    }
    let mut result = signature.ty;
    while let TypeRefKind::Fn { ret, .. } = &signature.types[result].kind {
        result = *ret;
    }
    let TypeRefKind::Var(var) = signature.types[result].kind else {
        return false;
    };
    u32::from(var.into_raw()) as usize >= fixed
        && !signature
            .constraints
            .iter()
            .any(|constraint| constraint.var == var)
        && occurrences(signature, signature.ty, var) == 1
}

/// 型 `ty` に型変数 `var` が現れる回数。矢印の row の中のエフェクトの型引数も数える。
fn occurrences(signature: &Signature, ty: TypeRefId, var: TypeVarId) -> usize {
    let types = &signature.types;
    let row = |row: &RowRef| match row {
        RowRef::Closed { effects, .. } | RowRef::Open { effects, .. } => effects
            .iter()
            .flat_map(|effect| &effect.args)
            .map(|&arg| occurrences(signature, arg, var))
            .sum(),
        RowRef::Omitted | RowRef::Error => 0,
    };
    match &types[ty].kind {
        TypeRefKind::Error => 0,
        TypeRefKind::Var(other) => usize::from(*other == var),
        TypeRefKind::Con(_, args, rows) => {
            args.iter()
                .map(|&arg| occurrences(signature, arg, var))
                .sum::<usize>()
                + rows.iter().map(row).sum::<usize>()
        }
        TypeRefKind::Fn {
            param,
            row: arrow,
            ret,
            ..
        } => occurrences(signature, *param, var) + row(arrow) + occurrences(signature, *ret, var),
        TypeRefKind::Tuple(elements) => elements
            .iter()
            .map(|&element| occurrences(signature, element, var))
            .sum(),
    }
}
