//! 利用者のCの関数のネイティブの呼び出し。どの実行のテストもVMの検査モードとネイティブで同じソースを流し、標準出力と
//! 結果の文が同じことを確かめる (docs/spec/native.mdの「Cの関数の呼び出し」)。
#![cfg(not(miri))]
use std::collections::{HashMap, HashSet};

use eml_native::{LoadError, NativeMachine};
use eml_nrt::RunConfig;

use crate::common::native::{agree_source, host};

const ADD_FITS: &str = "\
extern \"C\" pure symbol \"eml_fixture_add_i32\" add : Int as int -> Int as int -> <Panic> Int as int

main : Unit -> <IO, Panic> Unit
main () =
  println (show (add 2 3))
  println (show (add (0 - 2147483648) 2147483647))
";

const ADD_EFFECTFUL_TOO_BIG: &str = "\
extern \"C\" symbol \"eml_fixture_add_i32\" add : Int as int -> Int as int -> <IO, Panic> Int as int

main : Unit -> <IO, Panic> Unit
main () =
  println \"before\"
  println (show (add 1 2147483648))
";

const ADD_PURE_TOO_SMALL: &str = "\
extern \"C\" pure symbol \"eml_fixture_add_i32\" add : Int as int -> Int as int -> <Panic> Int as int

main : Unit -> <IO, Panic> Unit
main () =
  println \"before\"
  println (show (add (0 - 2147483649) 1))
";

const SUM12_NEGATIVE_SIZE: &str = "\
extern \"C\" pure symbol \"eml_fixture_sum12\" sum12 : Int as int -> Int -> Int as int -> Int as size_t -> Int as int -> Int as int -> Int as int -> Int as int -> Int as int -> Int as int -> Float -> Float -> <Panic> Int

main : Unit -> <IO, Panic> Unit
main () =
  println \"before\"
  println (show (sum12 1 2 3 (0 - 1) 5 6 7 8 9 10 0.5 0.25))
";

const SUM12_AND_ADD: &str = "\
extern \"C\" pure symbol \"eml_fixture_sum12\" sum12 : Int as int -> Int -> Int as int -> Int as size_t -> Int as int -> Int as int -> Int as int -> Int as int -> Int as int -> Int as int -> Float -> Float -> <Panic> Int
extern \"C\" pure symbol \"eml_fixture_add_i32\" add : Int as int -> Int as int -> <Panic> Int as int

main : Unit -> <IO, Panic> Unit
main () =
  let total = sum12 (0 - 1) 2 (0 - 3) 4 5 6 7 8 9 10 0.5 0.25
  let small = add (0 - 7) 3
  println (show total)
  println (show small)
  println (show (total + small))
";

const NEG: &str = "\
extern \"C\" pure symbol \"eml_fixture_neg\" neg : Unit -> Int as int
extern \"C\" pure symbol \"eml_fixture_add_i32\" add : Int as int -> Int as int -> <Panic> Int as int

main : Unit -> <IO, Panic> Unit
main () =
  println (show (neg ()))
  println (show (add 2147483647 1))
";

const FILL: &str = "\
import MutUnboxedArray

extern \"C\" pure symbol \"eml_fixture_fill\" fill : MutUnboxedArray Float -> Int -> Float -> (Unit, MutUnboxedArray Float)

main : Unit -> <IO, Panic> Unit
main () =
  let m = MutUnboxedArray.make 4 0.0
  let ((), m) = fill m 4 1.5
  let (x, m) = MutUnboxedArray.read m 3
  let (y, m) = MutUnboxedArray.read m 1
  drop m
  println (show x)
  println (show y)
";

const BYTES_SUM: &str = "\
import Bytes

extern \"C\" pure symbol \"eml_fixture_bytes_sum\" string_sum : String -> Int -> Int
extern \"C\" pure symbol \"eml_fixture_bytes_sum\" bytes_sum : Bytes -> Int -> Int

main : Unit -> <IO> Unit
main () =
  let s = \"a\" ++ \"bc\"
  println (show (string_sum s 3))
  println (show (string_sum \"xyz\" 2))
  println (show (bytes_sum (Bytes.of_string \"xyz\") 3))
";

const DOT: &str = "\
import MutUnboxedArray
import UnboxedArray

extern \"C\" pure symbol \"eml_fixture_dot\" dot : UnboxedArray Float -> UnboxedArray Float -> Int -> Float

main : Unit -> <IO, Panic> Unit
main () =
  let a = MutUnboxedArray.freeze (MutUnboxedArray.make 3 1.5)
  let b = MutUnboxedArray.freeze (MutUnboxedArray.make 3 4.0)
  println (show (dot a b 3))
  println (show (dot a b 2))
  println (show (dot a a 0))
";

const HANDLE: &str = "\
extern \"C\" data Handle drop \"eml_fixture_handle_free\"
extern \"C\" symbol \"eml_fixture_handle_new\" handle_new : Int -> Int -> <IO, Panic> Handle
extern \"C\" symbol \"eml_fixture_handle_get\" handle_get : Handle -> <IO> (Int, Handle)
extern \"C\" symbol \"eml_fixture_frees\" frees : Int -> <IO> Int

main : Unit -> <IO, Panic> Unit
main () =
  let h = handle_new 1 7
  let (v, h) = handle_get h
  let (w, h) = handle_get h
  drop h
  println (show (v + w))
  println (show (frees 1))
";

const NULL_HANDLE: &str = "\
extern \"C\" data Handle drop \"eml_fixture_handle_free\"
extern \"C\" symbol \"eml_fixture_handle_new\" handle_new : Int -> Int -> <IO, Panic> Handle
extern \"C\" symbol \"eml_fixture_handle_get\" handle_get : Handle -> <IO> (Int, Handle)

main : Unit -> <IO, Panic> Unit
main () =
  println \"before\"
  let h = handle_new 2 (0 - 1)
  let (v, h) = handle_get h
  drop h
  println (show v)
";

/// 実行のテストのソースのすべて。窓のテストがどれもコンパイルする。
const SOURCES: &[&str] = &[
    ADD_FITS,
    ADD_EFFECTFUL_TOO_BIG,
    ADD_PURE_TOO_SMALL,
    SUM12_NEGATIVE_SIZE,
    SUM12_AND_ADD,
    NEG,
    FILL,
    BYTES_SUM,
    DOT,
    HANDLE,
    NULL_HANDLE,
];

#[test]
fn an_int32_argument_that_fits() {
    agree_source(ADD_FITS);
}

#[test]
fn an_int32_argument_that_does_not_fit_panics() {
    agree_source(ADD_EFFECTFUL_TOO_BIG);
}

#[test]
fn a_pure_row_that_does_not_fit_panics() {
    agree_source(ADD_PURE_TOO_SMALL);
}

#[test]
fn a_negative_size_t_panics() {
    agree_source(SUM12_NEGATIVE_SIZE);
}

#[test]
fn twelve_mixed_arguments() {
    agree_source(SUM12_AND_ADD);
}

#[test]
fn an_int32_result_is_sign_extended() {
    agree_source(NEG);
}

#[test]
fn a_void_function_writes_through_a_lent_array() {
    agree_source(FILL);
}

#[test]
fn a_text_argument() {
    agree_source(BYTES_SUM);
}

#[test]
fn an_unboxed_array_argument() {
    agree_source(DOT);
}

#[test]
fn a_handle_round_trip_runs_the_dtor_once() {
    agree_source(HANDLE);
}

#[test]
fn a_null_handle_panics() {
    agree_source(NULL_HANDLE);
}

#[test]
fn a_linked_of_another_program_is_refused() {
    let program = eml_test_support::lir(ADD_FITS);
    let other = eml_test_support::lir(NEG);
    let mut machine = NativeMachine::new(&RunConfig::default(), host());
    assert_eq!(
        machine.load(&program, &eml_test_support::link(&other)),
        Err(LoadError::LinkMismatch)
    );
    let mut machine = NativeMachine::new(&RunConfig::default(), host());
    assert_eq!(
        machine.load(&program, &eml_cabi::Linked::default()),
        Err(LoadError::LinkMismatch)
    );
}

/// 貸す側の呼び出し。
const LENDING: &[&str] = &[
    "eml_rt_text_payload",
    "eml_rt_unboxed_ptr",
    "eml_rt_unboxed_ptr_mut",
    "eml_x_c_handle_ptr",
];

/// CLIFの1行の命令。
enum Inst<'a> {
    Call(&'a str),
    Brif(Vec<&'a str>),
    /// `call`のほかの呼び出しの形 (`call_indirect`、`return_call`など)。
    OtherCall,
    FaultLoad,
    Other,
}

/// 関数のCLIFの、ブロックごとの命令。ブロックはレイアウトの順である。
fn blocks(clif: &str) -> Vec<(&str, Vec<Inst<'_>>)> {
    let mut funcs: HashMap<&str, &str> = HashMap::new();
    let mut blocks: Vec<(&str, Vec<Inst>)> = Vec::new();
    for line in clif.lines() {
        let line = line.split(';').next().unwrap().trim();
        if let Some((name, rest)) = line.split_once(" = ")
            && name.starts_with("fn")
            && !name.contains(' ')
        {
            let symbol = rest.trim_start_matches("colocated ");
            let symbol = symbol.trim_start_matches('%');
            let symbol = symbol.split(' ').next().unwrap();
            funcs.insert(name, symbol);
        } else if line.starts_with("block") && line.ends_with(':') {
            let name = line.split(['(', ' ', ':']).next().unwrap();
            blocks.push((name, Vec::new()));
        } else if let Some((_, insts)) = blocks.last_mut() {
            let body = line.split_once(" = ").map_or(line, |(_, body)| body);
            let inst = if let Some(callee) = body.strip_prefix("call ") {
                let callee = callee.split('(').next().unwrap();
                Inst::Call(funcs[callee])
            } else if body.starts_with("call")
                || body.starts_with("return_call")
                || body.starts_with("try_call")
            {
                Inst::OtherCall
            } else if let Some(args) = body.strip_prefix("brif ") {
                let targets = args
                    .split(", ")
                    .skip(1)
                    .map(|target| target.split('(').next().unwrap())
                    .collect();
                Inst::Brif(targets)
            } else if body.starts_with("load.i64") && body.contains(" v0+32") {
                Inst::FaultLoad
            } else {
                Inst::Other
            };
            insts.push(inst);
        }
    }
    blocks
}

/// どの関数の、どのCの関数の呼び出しも、範囲の確かめを貸す呼び出しの前に済ませ、最初の貸す呼び出しからCの関数の
/// 呼び出しまでを1つのブロックの中で貸す呼び出しだけにし、Cの関数の呼び出しの後でfaultの語を、ハンドルを作った後の
/// ほかは読まない。
#[test]
fn the_lending_window_holds_only_lending_entries() {
    let (mut c_calls, mut lends, mut narrows, mut handles) = (0, 0, 0, 0);
    for source in SOURCES {
        let program = eml_test_support::lir(*source);
        let mut machine = NativeMachine::new_with_clif(&RunConfig::default(), host());
        machine
            .load(&program, &eml_test_support::link(&program))
            .unwrap_or_else(|error| panic!("{error:?}"));
        for function in &program.functions {
            let clif = machine
                .clif(&function.name)
                .expect("every function's CLIF is kept");
            let blocks = blocks(clif);
            let narrowing: HashSet<&str> = blocks
                .iter()
                .filter(|(_, insts)| {
                    insts
                        .iter()
                        .any(|inst| matches!(inst, Inst::Call("eml_x_c_narrow")))
                })
                .map(|(name, _)| *name)
                .collect();
            let mut lent = false;
            for (block, insts) in &blocks {
                assert!(
                    !lent,
                    "{}: a lending window crosses into {block}\n{clif}",
                    function.name
                );
                let mut after_c = false;
                let mut after_handle_new = false;
                for inst in insts {
                    if after_handle_new {
                        assert!(
                            matches!(inst, Inst::FaultLoad),
                            "{}: the fault word is not read right after the handle\n{clif}",
                            function.name
                        );
                        after_handle_new = false;
                        handles += 1;
                        continue;
                    }
                    match inst {
                        Inst::Brif(targets) => {
                            if targets.iter().any(|target| narrowing.contains(target)) {
                                narrows += 1;
                                assert!(
                                    !lent,
                                    "{}: a narrowing check follows a lending call\n{clif}",
                                    function.name
                                );
                            }
                        }
                        Inst::Call(symbol) if LENDING.contains(symbol) => {
                            lends += 1;
                            lent = true;
                        }
                        Inst::Call(symbol) if symbol.starts_with("eml_c$") => {
                            c_calls += 1;
                            lent = false;
                            after_c = true;
                            continue;
                        }
                        Inst::Call(symbol) => {
                            assert!(
                                !lent,
                                "{}: `{symbol}` runs inside a lending window\n{clif}",
                                function.name
                            );
                            after_handle_new = after_c && *symbol == "eml_x_c_handle_new";
                            after_c = false;
                            continue;
                        }
                        Inst::OtherCall => {
                            assert!(
                                !lent,
                                "{}: an indirect or tail call runs inside a lending window\n{clif}",
                                function.name
                            );
                            after_c = false;
                        }
                        Inst::FaultLoad => assert!(
                            !after_c,
                            "{}: the fault word is read after a C call\n{clif}",
                            function.name
                        ),
                        Inst::Other => {}
                    }
                }
                assert!(
                    !after_handle_new,
                    "{}: the block ends right after the handle\n{clif}",
                    function.name
                );
            }
        }
    }
    assert!(c_calls >= SOURCES.len(), "{c_calls} C calls");
    assert!(
        lends > 0 && narrows > 0 && handles > 0,
        "{lends} lending calls, {narrows} checks, {handles} handles"
    );
}
