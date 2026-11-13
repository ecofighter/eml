use eml_extern::Extern;
use eml_native::rows::{INLINE, Lowering, lowering};
use eml_native::{Form, Unsupported, unsupported};

use crate::common::native::program;

fn rejected(source: &str) -> Vec<Unsupported> {
    unsupported(&eml_test_support::lir(source))
}

fn forms(source: &str) -> Vec<Form> {
    rejected(source).iter().map(|u| u.form).collect()
}

fn assert_rejects(source: &str, form: Form) {
    let found = forms(source);
    assert!(found.contains(&form), "{form:?} not in {found:?}");
}

#[test]
fn par_is_rejected() {
    assert_rejects(
        "import Std.Par\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () =\n  let (a, b) = Par.par (fn () -> 1) (fn () -> 2)\n  println (show (a + b))\n",
        Form::Par,
    );
}

#[test]
fn async_row_is_rejected() {
    assert_rejects(
        "import Async\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () = println (show (Async.run (fn () -> 1)))\n",
        Form::AsyncRow,
    );
}

#[test]
fn blocking_c_is_rejected() {
    assert_rejects(
        "import Async (Async)\n\
         \n\
         extern \"C\" blocking symbol \"eml_fixture_add_i32\" add : Int as int -> Int as int -> <IO, Panic, Async> Int as int\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () = Async.run (fn () -> println (show (add 1 2)))\n",
        Form::BlockingC,
    );
}

#[test]
fn prim_is_rejected() {
    assert_rejects(
        "effect Fail where\n  never fail : String -> a\n\
         \n\
         check : Int -> <Fail> Int\n\
         check n = if n > 0 then n else fail \"not positive\"\n\
         \n\
         safe : Int -> String\n\
         safe n =\n  handle show (check n) with\n    | ctl fail message -> message\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () =\n  println (safe 3)\n  println (safe 0)\n",
        Form::Prim,
    );
}

#[test]
fn clause_is_rejected() {
    assert_rejects(
        "main : Unit -> <IO> Unit\n\
         main () = println (show (try_panic (fn () -> 9223372036854775807 + 1)))\n",
        Form::Clause,
    );
}

#[test]
fn prompt_is_rejected() {
    assert_rejects(
        "effect Fail where\n  never fail : String -> a\n\
         \n\
         check : Int -> <Fail> Int\n\
         check n = if n > 0 then n else fail \"not positive\"\n\
         \n\
         safe : Int -> String\n\
         safe n =\n  handle show (check n) with\n    | ctl fail message -> message\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () =\n  println (safe 3)\n  println (safe 0)\n",
        Form::Prompt,
    );
}

#[test]
fn c_row_is_rejected() {
    assert_rejects(
        "extern \"C\" pure symbol \"eml_fixture_add_i32\" add : Int as int -> Int as int -> <Panic> Int as int\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () = println (show (add 2 3))\n",
        Form::CRow,
    );
}

#[test]
fn forms_are_sorted_and_distinct() {
    let found = forms(
        "import Std.Par\n\
         \n\
         effect Fail where\n  never fail : String -> a\n\
         \n\
         check : Int -> <Fail> Int\n\
         check n = if n > 0 then n else fail \"not positive\"\n\
         \n\
         safe : Int -> String\n\
         safe n =\n  handle show (check n) with\n    | ctl fail message -> message\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () =\n  let (a, b) = Par.par (fn () -> safe 1) (fn () -> safe 0)\n  println (a ++ b)\n",
    );
    for form in [Form::Par, Form::Prim, Form::Prompt] {
        assert!(found.contains(&form), "{form:?} not in {found:?}");
    }
    assert!(found.windows(2).all(|pair| pair[0] < pair[1]), "{found:?}");
}

/// 関数は宣言の順に並ぶ。`Std.Atomic.add`は`bump`の最初のタスクの行で、`fresh`の`Std.Atomic.new`はその後の関数に
/// ある。emlのソースでは`Std.Async`の関数が先に並ぶので、LIRのテキストで書く。
#[test]
fn the_first_function_and_row_are_reported() {
    let found = unsupported(&program(
        "\
fn bump(c.0: int) -> int {
  let a.1: int = extern Std.Atomic.add(1, c.0)
  let b.2: int = extern Std.Atomic.load(c.0)
  return b.2
}
fn fresh(n.0: int) -> int {
  let c.1: int = extern Std.Atomic.new(n.0)
  return c.1
}
fn entry$main() -> unit {
  return ()
}
",
    ));
    assert_eq!(
        found,
        [Unsupported {
            form: Form::AsyncRow,
            function: "bump".to_owned(),
            row: Some("Std.Atomic.add"),
        }]
    );
}

/// タスクの行と`Par`のほかのどの行も、インラインに展開するか呼ぶかで下ろす。
#[test]
fn inline_and_called_rows_are_supported() {
    for &row in Extern::ALL {
        let expected = if row.is_async() || row == Extern::Par {
            None
        } else if let Some(&(_, inline)) = INLINE.iter().find(|(r, _)| *r == row) {
            Some(Lowering::Inline(inline))
        } else {
            Some(Lowering::Called)
        };
        assert_eq!(lowering(row), expected, "{row:?}");
    }
    assert_eq!(
        rejected(
            "import Float\n\
             \n\
             main : Unit -> <IO, Panic> Unit\n\
             main () =\n  println (show (1 + 2 * 3))\n  println (show 1.5 ++ show (2.5 % 2.0))\n  println (show (Float.truncate 9.5))\n",
        ),
        []
    );
}

#[test]
fn the_inline_table_is_the_vm_arithmetic_list_without_float_mod() {
    let mut rows: Vec<Extern> = INLINE.iter().map(|&(row, _)| row).collect();
    let mut expected = vec![
        Extern::IntAdd,
        Extern::IntSub,
        Extern::IntMul,
        Extern::IntDiv,
        Extern::ByteDiv,
        Extern::IntMod,
        Extern::ByteMod,
        Extern::IntNeg,
        Extern::IntEq,
        Extern::IntNe,
        Extern::IntLt,
        Extern::IntLe,
        Extern::IntGt,
        Extern::IntGe,
        Extern::CharEq,
        Extern::CharNe,
        Extern::CharLt,
        Extern::CharLe,
        Extern::CharGt,
        Extern::CharGe,
        Extern::ByteEq,
        Extern::ByteNe,
        Extern::ByteLt,
        Extern::ByteLe,
        Extern::ByteGt,
        Extern::ByteGe,
        Extern::IntCompare,
        Extern::CharCompare,
        Extern::ByteCompare,
        Extern::CharToInt,
        Extern::ByteToInt,
        Extern::CharOfByte,
        Extern::FloatAdd,
        Extern::FloatSub,
        Extern::FloatMul,
        Extern::FloatDiv,
        Extern::FloatNeg,
        Extern::FloatEq,
        Extern::FloatNe,
        Extern::FloatLt,
        Extern::FloatLe,
        Extern::FloatGt,
        Extern::FloatGe,
        Extern::BoolEq,
        Extern::BoolNe,
    ];
    assert_eq!(expected.len(), 45);
    rows.sort_by_key(|row| *row as usize);
    expected.sort_by_key(|row| *row as usize);
    assert_eq!(rows, expected);
}
