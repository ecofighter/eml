use eml_hir::builtin::Builtin;

use crate::table::{Row, Table, Ty};
use crate::ty::Effect;

/// 組み込みの型。型と意味は docs/spec/declarations.md の標準の演算子の表と、docs/spec/effects.md の組み込みの
/// `IO` にある。
pub(crate) fn builtin_type(table: &mut Table, builtin: Builtin) -> Ty {
    let (int, string, bool, unit) = (table.int, table.string, table.bool, table.unit);
    match builtin {
        Builtin::Println => table.function(string, Row::closed(vec![Effect::Io]), unit),
        Builtin::ShowInt => table.function(int, Row::pure(), string),
        Builtin::Not => table.function(bool, Row::pure(), bool),
        Builtin::True | Builtin::False => bool,
        Builtin::IntNeg => table.function(int, Row::pure(), int),
        Builtin::IntAdd | Builtin::IntSub | Builtin::IntMul | Builtin::IntDiv | Builtin::IntMod => {
            binary(table, int, int)
        }
        Builtin::IntEq
        | Builtin::IntNe
        | Builtin::IntLt
        | Builtin::IntLe
        | Builtin::IntGt
        | Builtin::IntGe => binary(table, int, bool),
        Builtin::StrConcat => binary(table, string, string),
        Builtin::ComposeFwd | Builtin::ComposeBwd => compose(table, builtin == Builtin::ComposeFwd),
    }
}

fn binary(table: &mut Table, operand: Ty, result: Ty) -> Ty {
    let inner = table.function(operand, Row::pure(), result);
    table.function(operand, Row::pure(), inner)
}

/// `f >> g` と `g << f` は、どちらも `fn x -> g (f x)` と同じ関数になる。被演算子は左から右に評価するので、
/// 引数の並びだけが違う2つの組み込みにする (docs/spec/declarations.md の標準の演算子の表)。
fn compose(table: &mut Table, forward: bool) -> Ty {
    let (a, b, c) = (table.fresh_var(), table.fresh_var(), table.fresh_var());
    let e = table.fresh_row_var();
    let effectful = |table: &mut Table, from: Ty, to: Ty| {
        let m = table.fresh_mult();
        table.function_with(
            from,
            m,
            Row {
                labels: Vec::new(),
                tail: Some(e),
            },
            to,
        )
    };
    let f = effectful(table, a, b);
    let g = effectful(table, b, c);
    let result = effectful(table, a, c);
    let (first, second) = if forward { (f, g) } else { (g, f) };
    let m = table.fresh_mult();
    let inner = table.function_with(second, m, Row::pure(), result);
    let outer = table.function(first, Row::pure(), inner);
    table.closure_kinds(outer, 3, &[]);
    outer
}
