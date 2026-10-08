//! extern の表の行の Repr と、std の宣言の型の Repr の照らし合わせ (docs/spec/core-ir.md)。行の名前と矢印の数は
//! `eml_hir` の結合テストが照らし合わせる。型から Repr を決める規則は translate にあるので、ここで確かめる。

use eml_core_ir::type_repr;
use eml_extern::{Extern, ExternType};
use eml_hir::{Function, FunctionKind, ValueItem};
use eml_test_support::Checked;
use eml_types::Type;

/// 型の行ごとに、その型の値をそのまま返す関数を置く。型検査がその型に与える型を、関数の引数の型から読む。
const PROBES: &str = "\
import Std.Fs

int : Int -> Int
int x = x

string : String -> String
string s = s

unit : Unit -> Unit
unit u = u

file : Fs.File -> Fs.File
file f = f

main : Unit -> <IO> Unit
main () = println \"x\"
";

/// 型の行を読む `PROBES` の関数。`_` の腕を書かないので、型の行を足すとここがコンパイルできなくなる。
fn probe(ty: ExternType) -> &'static str {
    match ty {
        ExternType::Int => "int",
        ExternType::String => "string",
        ExternType::Unit => "unit",
        ExternType::File => "file",
    }
}

fn checked() -> Checked {
    let checked = eml_test_support::check(PROBES);
    assert!(
        checked.diagnostics.is_empty(),
        "{}",
        eml_test_support::short_text(checked.files(), &checked.diagnostics)
    );
    checked
}

/// `wanted` に合う関数の宣言の型。
fn declared(checked: &Checked, wanted: impl Fn(&Function) -> bool) -> &Type {
    let (id, _) = checked
        .program
        .functions()
        .find(|(_, function)| wanted(function))
        .expect("the function is declared");
    &checked.typed.decls[&ValueItem::Function(id)].ty
}

fn extern_type(checked: &Checked, e: Extern) -> &Type {
    declared(checked, |function| {
        function.kind == FunctionKind::Extern(Some(e))
    })
}

fn has_type_var(ty: &Type) -> bool {
    match ty {
        Type::Con { id: _, args } => args.iter().any(has_type_var),
        Type::Record(fields) => fields.iter().any(|(_, ty)| has_type_var(ty)),
        Type::Fn {
            param,
            effects: _,
            tail: _,
            ret,
        } => has_type_var(param) || has_type_var(ret),
        Type::Rigid(_) | Type::Flexible => true,
        Type::Error => false,
    }
}

#[test]
fn every_extern_function_row_has_the_reprs_of_its_std_signature() {
    let checked = checked();
    for &e in Extern::ALL {
        let row = e.row();
        let mut ty = extern_type(&checked, e);
        let mut params = Vec::new();
        for _ in row.params {
            let Type::Fn { param, ret, .. } = ty else {
                panic!("`{}` has an arrow per parameter", row.name);
            };
            params.push(type_repr(param, &checked.program));
            ty = ret;
        }
        assert_eq!(params, row.params, "{}", row.name);
        assert_eq!(type_repr(ty, &checked.program), row.ret, "{}", row.name);
    }
}

#[test]
fn every_extern_type_row_has_the_repr_of_its_type() {
    // 型検査は `Unit` を空のレコードにするので、`Prelude.Unit` の行は `types.rs` の空のレコードの規則と比べる
    let checked = checked();
    for &ty in ExternType::ALL {
        let row = ty.row();
        let name = probe(ty);
        let defined =
            |function: &Function| function.kind == FunctionKind::Defined && function.name == name;
        let Type::Fn { param, .. } = declared(&checked, defined) else {
            panic!("`{name}` is a function");
        };
        assert_eq!(type_repr(param, &checked.program), row.repr, "{}", row.name);
    }
}

#[test]
fn extern_function_rows_not_chosen_by_type_are_monomorphic() {
    // 行の Repr は型変数の位置を `tobj` として比べる。多相な extern は S4 で入り、そこで比べ方を決め直す
    // (docs/future/roadmap.md)
    let checked = checked();
    for &e in Extern::ALL {
        let row = e.row();
        if row.by_type {
            continue;
        }
        assert!(!has_type_var(extern_type(&checked, e)), "{}", row.name);
    }
}
