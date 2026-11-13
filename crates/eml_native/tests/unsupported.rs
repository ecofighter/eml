use eml_extern::{Extern, OpRet, Repr};
use eml_native::rows::{CALLED, INLINE, Lowering, lowering};
use eml_native::{Form, Unsupported, unsupported};
use eml_nrt::Kind;

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
fn std_row_is_rejected() {
    let found = rejected(
        "import Float\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () = println (show (Float.truncate 9.5))\n",
    );
    assert_eq!(
        found,
        [Unsupported {
            form: Form::StdRow,
            function: "entry$main".to_owned(),
            row: Some("Std.Float.truncate"),
        }]
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
fn closure_is_rejected() {
    assert_rejects(
        "count_down : Int -> (Int -> <Panic> Int) -> <Panic> Int\n\
         count_down n k = if n == 0 then k 0 else count_down (n - 1) (fn m -> k (m + 1))\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () = println (show (count_down 10 (fn m -> m)))\n",
        Form::Closure,
    );
}

#[test]
fn fn_value_is_rejected() {
    assert_rejects(
        "count_down : Int -> (Int -> <Panic> Int) -> <Panic> Int\n\
         count_down n k = if n == 0 then k 0 else count_down (n - 1) (fn m -> k (m + 1))\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () = println (show (count_down 10 (fn m -> m)))\n",
        Form::FnValue,
    );
}

#[test]
fn apply_is_rejected() {
    assert_rejects(
        "count_down : Int -> (Int -> <Panic> Int) -> <Panic> Int\n\
         count_down n k = if n == 0 then k 0 else count_down (n - 1) (fn m -> k (m + 1))\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () = println (show (count_down 10 (fn m -> m)))\n",
        Form::Apply,
    );
}

#[test]
fn forms_are_sorted_and_distinct() {
    let found = forms(
        "import Std.Par\nimport Float\n\
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
         main () =\n  let (a, b) = Par.par (fn () -> safe 1) (fn () -> safe 0)\n  println (a ++ b)\n  println (show (Float.truncate 9.5))\n",
    );
    for form in [Form::Par, Form::Prim, Form::StdRow] {
        assert!(found.contains(&form), "{form:?} not in {found:?}");
    }
    assert!(found.windows(2).all(|pair| pair[0] < pair[1]), "{found:?}");
}

/// 関数は宣言の順に並ぶ。`Float.truncate`は`sum_floors`の最初の行で、`roots`の`Float.sqrt`はその後の関数にある。
#[test]
fn the_first_function_and_row_are_reported() {
    let found = rejected(
        "import Float\n\
         \n\
         sum_floors : Int -> Float -> <Panic> Int\n\
         sum_floors n x = if n == 0 then 0 else Float.truncate x + Float.round x + sum_floors (n - 1) x\n\
         \n\
         roots : Int -> Float -> <Panic> Float\n\
         roots n x = if n == 0 then x else roots (n - 1) (Float.sqrt x)\n\
         \n\
         main : Unit -> <IO, Panic> Unit\n\
         main () =\n  println (show (sum_floors 3 2.5))\n  println (show (roots 2 81.0))\n",
    );
    assert_eq!(
        found,
        [Unsupported {
            form: Form::StdRow,
            function: "sum_floors".to_owned(),
            row: Some("Std.Float.truncate"),
        }]
    );
}

#[test]
fn inline_and_called_rows_are_supported() {
    for &(row, inline) in INLINE {
        assert_eq!(lowering(row), Some(Lowering::Inline(inline)), "{row:?}");
    }
    for &row in CALLED {
        assert_eq!(lowering(row), Some(Lowering::Called), "{row:?}");
    }
    assert_eq!(lowering(Extern::FloatTruncate), None);
    assert_eq!(
        rejected(
            "main : Unit -> <IO, Panic> Unit\n\
             main () =\n  println (show (1 + 2 * 3))\n  println (show 1.5 ++ show (2.5 % 2.0))\n",
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

#[test]
fn the_called_rows_are_the_seed() {
    assert_eq!(
        CALLED,
        [
            Extern::Println,
            Extern::Eprintln,
            Extern::IntShow,
            Extern::StrShow,
            Extern::FloatShow,
            Extern::StrConcat,
            Extern::StrEq,
            Extern::StrCompare,
            Extern::FloatMod,
            Extern::Exit,
            Extern::Panic,
        ]
    );
}

fn kind_of(repr: Repr) -> Kind {
    match repr {
        Repr::Float => Kind::F64,
        _ => Kind::I64,
    }
}

#[test]
fn every_called_row_has_an_entry() {
    for &row in CALLED {
        let symbol = format!("eml_x_{row:?}");
        let entry = eml_nrt::entries()
            .find(|e| e.symbol == symbol)
            .unwrap_or_else(|| panic!("no entry {symbol}"));
        let desc = row.row();
        let params: Vec<Kind> = desc.params.iter().map(|&r| kind_of(r)).collect();
        let result = match desc.ret {
            OpRet::Never => None,
            OpRet::Repr(r) => Some(kind_of(r)),
        };
        assert_eq!(
            (entry.params, entry.result),
            (&params[..], result),
            "{symbol}"
        );
    }
}

#[test]
fn the_inline_table_is_the_marked_set() {
    let mut marked: Vec<Extern> = eml_nrt::LISTED
        .iter()
        .filter(|l| l.inline)
        .map(|l| l.row)
        .collect();
    let mut rows: Vec<Extern> = INLINE.iter().map(|&(row, _)| row).collect();
    marked.sort_by_key(|row| *row as usize);
    rows.sort_by_key(|row| *row as usize);
    assert_eq!(rows, marked);
}
