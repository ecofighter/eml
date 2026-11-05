//! `extern "C"` の型の対応と効果の行 (docs/spec/declarations.md の「型の対応」)。

use eml_extern::CInt;
use eml_test_support::check;
use eml_types::{CArg, CResult, CShape};

use crate::common::check_full;

/// エントリのモジュールの C の関数 `name` の形と、型検査までの診断。
fn shape(text: &str, name: &str) -> (Option<CShape>, Vec<String>) {
    let checked = check(text);
    let (id, _) = checked
        .program
        .functions()
        .find(|(id, function)| id.module == checked.program.entry && function.name == name)
        .unwrap_or_else(|| panic!("`{name}` is declared"));
    let shape = checked.typed.c_shapes.get(&id).cloned();
    (
        shape,
        eml_test_support::short(checked.files(), &checked.diagnostics),
    )
}

/// エントリのモジュールの型 `name` の ID。
fn handle(text: &str, name: &str) -> eml_hir::TypeDefId {
    let checked = check(text);
    checked
        .program
        .types()
        .find(|(id, def)| id.module == checked.program.entry && def.name == name)
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("`{name}` is declared"))
}

#[test]
fn the_mapped_types_have_their_shapes() {
    let text = [
        "extern \"C\" data Solver",
        "extern \"C\" f : Int as int -> Int as size_t -> Float -> UnboxedArray Float -> UnboxedArray Int -> String -> Bytes -> <IO, Panic> Float",
        "extern \"C\" g : MutUnboxedArray Float -> MutUnboxedArray Int -> Solver -> <IO> (Int as int, MutUnboxedArray Float, MutUnboxedArray Int, Solver)",
        "extern \"C\" pure h : Unit -> Int",
    ]
    .join("\n");
    let solver = handle(&text, "Solver");
    assert_eq!(
        shape(&text, "f"),
        (
            Some(CShape {
                params: vec![
                    CArg::Int(CInt::I32),
                    CArg::Int(CInt::SizeT),
                    CArg::Float,
                    CArg::UnboxedArray,
                    CArg::UnboxedArray,
                    CArg::Text,
                    CArg::Text,
                ],
                ret: CResult::Float,
            }),
            vec![]
        )
    );
    assert_eq!(
        shape(&text, "g"),
        (
            Some(CShape {
                params: vec![
                    CArg::MutUnboxedArray,
                    CArg::MutUnboxedArray,
                    CArg::Handle(solver),
                ],
                ret: CResult::Int(CInt::I32),
            }),
            vec![]
        )
    );
    assert_eq!(
        shape(&text, "h"),
        (
            Some(CShape {
                params: vec![CArg::NoArg],
                ret: CResult::Int(CInt::I64),
            }),
            vec![]
        )
    );
}

#[test]
fn widths_default_to_int64() {
    let text =
        "extern \"C\" f : Int -> Int as int64_t -> <IO> Int\nextern \"C\" g : Int -> <IO> Unit";
    assert_eq!(
        shape(text, "f"),
        (
            Some(CShape {
                params: vec![CArg::Int(CInt::I64), CArg::Int(CInt::I64)],
                ret: CResult::Int(CInt::I64),
            }),
            vec![]
        )
    );
    assert_eq!(
        shape(text, "g").0.map(|shape| shape.ret),
        Some(CResult::Void)
    );
}

#[test]
fn a_handle_result_is_accepted() {
    let text = "extern \"C\" data Solver drop \"solver_free\"\nextern \"C\" open : Int -> <IO, Panic> Solver";
    let solver = handle(text, "Solver");
    assert_eq!(
        shape(text, "open"),
        (
            Some(CShape {
                params: vec![CArg::Int(CInt::I64)],
                ret: CResult::Handle(solver),
            }),
            vec![]
        )
    );
}

#[test]
fn a_handle_result_with_lin_arguments_is_accepted() {
    // 結果の `R` の位置のハンドルは新しい物体で、貸した引数の後ろの位置とは別である
    let text = [
        "extern \"C\" data Solver",
        "extern \"C\" split : Solver -> MutUnboxedArray Float -> <IO, Panic> (Solver, Solver, MutUnboxedArray Float)",
    ]
    .join("\n");
    let solver = handle(&text, "Solver");
    assert_eq!(
        shape(&text, "split"),
        (
            Some(CShape {
                params: vec![CArg::Handle(solver), CArg::MutUnboxedArray],
                ret: CResult::Handle(solver),
            }),
            vec![]
        )
    );
}

#[test]
fn a_type_variable_is_e2022() {
    insta::assert_snapshot!(check_full("extern \"C\" f : a -> <IO> Int"), @"
    E2022 1:16 `a` cannot cross to C here
      1:16 a C function has one fixed type at each position
    ");
}

#[test]
fn a_function_type_is_e2022() {
    insta::assert_snapshot!(check_full("extern \"C\" f : (Int -> Int) -> <IO> Int"), @"
    E2022 1:17 `Int -> Int` cannot cross to C here
      1:17 C functions cannot take or return functions
    ");
}

#[test]
fn bool_is_e2022_with_a_hint() {
    insta::assert_snapshot!(check_full("extern \"C\" f : Bool -> <IO> Bool"), @"
    E2022 1:16 `Bool` cannot cross to C here
      1:16 use `Int`
    E2022 1:29 `Bool` cannot cross to C here
      1:29 use `Int`
    ");
}

#[test]
fn a_string_result_is_e2022() {
    insta::assert_snapshot!(check_full("extern \"C\" f : Int -> <IO> String"), @"
    E2022 1:28 `String` cannot cross to C here
      1:28 a C function cannot return text; return a length and fill a `MutUnboxedArray` instead
    ");
}

#[test]
fn an_array_result_is_e2022() {
    insta::assert_snapshot!(check_full("extern \"C\" f : Int -> <IO> UnboxedArray Float"), @"
    E2022 1:28 `UnboxedArray Float` cannot cross to C here
      1:28 a C function cannot return an array; pass a `MutUnboxedArray` for it to fill
    ");
}

#[test]
fn as_on_a_float_is_e2022() {
    insta::assert_snapshot!(check_full("extern \"C\" f : Float as int -> <IO> Float"), @"
    E2022 1:22 `Float as int32_t` cannot cross to C here
      1:22 `as` gives the C width of an `Int` argument or result
    ");
}

#[test]
fn as_inside_an_array_type_is_e2022() {
    insta::assert_snapshot!(
        check_full("extern \"C\" f : UnboxedArray (Int as int) -> <IO> Unit"),
        @"
    E2022 1:34 `Int as int32_t` cannot cross to C here
      1:34 `as` gives the C width of an `Int` argument or result
    "
    );
}

#[test]
fn a_second_unit_parameter_is_e2022() {
    insta::assert_snapshot!(check_full("extern \"C\" f : Unit -> Int -> <IO> Int"), @"
    E2022 1:16 `Unit` cannot cross to C here
      1:16 `Unit` is the only argument of a C function without arguments
    ");
}

#[test]
fn a_result_without_the_lin_arguments_is_e2022() {
    let text = [
        "extern \"C\" fill : MutUnboxedArray Float -> <IO> Unit",
        "extern \"C\" swap : MutUnboxedArray Float -> MutUnboxedArray Int -> <IO> (Unit, MutUnboxedArray Int, MutUnboxedArray Float)",
        "extern \"C\" extra : Int -> <IO> (Unit, Int)",
    ]
    .join("\n");
    insta::assert_snapshot!(check_full(&text), @"
    E2022 1:49 `Unit` cannot cross to C here
      1:49 a C function gives back its `MutUnboxedArray` and handle arguments after its result: write `(Unit, MutUnboxedArray Float)`
    E2022 2:79 `MutUnboxedArray Int` cannot cross to C here
      2:79 this position gives back the argument `MutUnboxedArray Float`
    E2022 2:100 `MutUnboxedArray Float` cannot cross to C here
      2:100 this position gives back the argument `MutUnboxedArray Int`
    E2022 3:32 `(Unit, Int)` cannot cross to C here
      3:32 a C function returns a tuple only to give back its `MutUnboxedArray` and handle arguments, and this one has none
    ");
}

#[test]
fn a_narrowing_function_needs_panic() {
    insta::assert_snapshot!(check_full("extern \"C\" f : Int as int -> <IO> Int"), @"
    E2023 1:30 the effects of `f` must be `<IO, Panic>`
      1:30 found `<IO>`
      note: `IO` because the function is not `pure`
      note: `Panic` because an `Int` argument that does not fit its C type panics
    ");
}

#[test]
fn a_handle_result_needs_panic() {
    let text = "extern \"C\" data Solver\nextern \"C\" pure open : Int -> Solver";
    insta::assert_snapshot!(check_full(text), @"
    E2023 2:24 the effects of `open` must be `<Panic>`
      2:24 found `<>`
      note: `Panic` because a null handle from C panics
    ");
}

#[test]
fn a_pure_function_has_no_io() {
    insta::assert_snapshot!(check_full("extern \"C\" pure cos : Float -> <IO> Float"), @"
    E2023 1:32 the effects of `cos` must be `<>`
      1:32 found `<IO>`
      note: a `pure` C function without narrowing `Int` arguments or a handle result has no effects
    ");
}

#[test]
fn an_inner_row_is_e2023() {
    insta::assert_snapshot!(check_full("extern \"C\" f : Int -> <IO> Int -> <IO> Int"), @"
    E2023 1:23 the effects of `f` must be `<IO>`
      1:23 only the last arrow of a C function has effects
    ");
}

#[test]
fn a_blocking_row_needs_io_and_async() {
    let correct = "import Async (Async)\n\nextern \"C\" blocking wait : Int -> <IO, Async> Int";
    insta::assert_snapshot!(check_full(correct), @"");
    let wrong = "import Async (Async)\n\nextern \"C\" blocking wait : Int -> <IO> Int";
    insta::assert_snapshot!(check_full(wrong), @"
    E2023 3:35 the effects of `wait` must be `<IO, Async>`
      3:35 found `<IO>`
      note: `IO` because the function is not `pure`
      note: `Async` because the function is `blocking`
    ");
}
