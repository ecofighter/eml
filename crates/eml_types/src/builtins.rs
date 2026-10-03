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
    }
}

fn binary(table: &mut Table, operand: Ty, result: Ty) -> Ty {
    let inner = table.function(operand, Row::pure(), result);
    table.function(operand, Row::pure(), inner)
}
