//! JITのホスト`NativeMachine`と、呼び出しのない関数の下ろし (docs/spec/native.md の「関数の形」)。JITしたコードに
//! エントリのstubを通して入るので、Miriでは流さない。
#![cfg(not(miri))]
use eml_cabi::Linked;
use eml_native::{Arg, LoadError};

use crate::common::native::{agree, agree_lowered, call, checked, clif, machine, program};

const MAIN: &str = "fn entry$main() -> unit {\n  return ()\n}\n";

#[test]
fn a_function_returns_its_argument() {
    let text = "fn f(x.0: int) -> int {\n  return x.0\n}\n";
    for n in [0, 42, -7, i64::MIN, i64::MAX] {
        assert_eq!(call(text, "f", &[Arg::I64(n)]), [Arg::I64(n)]);
    }
    let text = format!("{MAIN}fn g(x.0: float) -> float {{\n  return x.0\n}}\n");
    assert_eq!(call(&text, "g", &[Arg::F64(-2.5)]), [Arg::F64(-2.5)]);
}

#[test]
fn many_values_return_in_order() {
    let text = format!(
        "{MAIN}\
         fn f(a.0: int, x.1: float) -> (int, float, int) {{\n  return (7, x.1, a.0)\n}}\n\
         fn g() -> (enum, tobj, unit) {{\n  return (#1, #2, ())\n}}\n"
    );
    assert_eq!(
        call(&text, "f", &[Arg::I64(3), Arg::F64(1.5)]),
        [Arg::I64(7), Arg::F64(1.5), Arg::I64(3)]
    );
    // タグは`enum`の位置ではその番号、`tobj`の位置では即値の語で、Unitは値0の即値である
    assert_eq!(
        call(&text, "g", &[]),
        [Arg::I64(1), Arg::I64((2 << 1) | 1), Arg::I64(1)]
    );
    let f = clif(&text, "f");
    assert!(f.contains("(i64, i64, f64) -> i64, f64, i64 tail"), "{f}");
}

#[test]
fn a_jump_passes_block_arguments() {
    let text = format!(
        "{MAIN}\
         fn f(a.0: int, b.1: int, x.2: float) -> (int, int, float) {{\n  jump b1(b.1, a.0, 2.5)\n\
         b1(c.3: int, d.4: int, y.5: float):\n  jump b2(y.5, c.3, d.4)\n\
         b2(z.6: float, e.7: int, g.8: int):\n  return (e.7, g.8, z.6)\n}}\n"
    );
    assert_eq!(
        call(&text, "f", &[Arg::I64(1), Arg::I64(2), Arg::F64(0.5)]),
        [Arg::I64(2), Arg::I64(1), Arg::F64(2.5)]
    );
}

#[test]
fn an_int_switch_takes_the_default() {
    let text = "\
layout Prelude.Bool { False, True }
fn f(n.0: int) -> int {
  switch n.0 { -3 -> b1, 1 -> b2, 2 -> b3, _ -> b4 }
b1:
  return 30
b2:
  return 10
b3:
  return 11
b4:
  return n.0
}
fn g(c.1: enum) -> int {
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  return 20
b2:
  return 21
}
fn h() -> int {
  switch 2 { 1 -> b1, _ -> b2 }
b1:
  return 1
b2:
  return 2
}
";
    for (n, expected) in [
        (-3, 30),
        (1, 10),
        (2, 11),
        (0, 0),
        (5, 5),
        (i64::MIN, i64::MIN),
    ] {
        assert_eq!(call(text, "f", &[Arg::I64(n)]), [Arg::I64(expected)]);
    }
    assert_eq!(call(text, "g", &[Arg::I64(0)]), [Arg::I64(20)]);
    assert_eq!(call(text, "g", &[Arg::I64(1)]), [Arg::I64(21)]);
    // 定数のscrutineeは、コード生成の時点で行き先を決める
    assert_eq!(call(text, "h", &[]), [Arg::I64(2)]);
}

#[test]
fn an_entry_with_two_results_is_refused() {
    for text in [
        "fn entry$main() -> (int, int) {\n  return (1, 2)\n}\n",
        "fn entry$main() -> float {\n  return 1.5\n}\n",
    ] {
        let mut m = machine();
        assert_eq!(
            m.load(&program(text), &Linked::default()),
            Err(LoadError::Entry {
                function: "entry$main".to_string()
            })
        );
        m.close();
    }
}

#[test]
fn a_second_load_is_refused() {
    let text = format!("{MAIN}fn f(x.0: int) -> int {{\n  return x.0\n}}\n");
    let program = program(&text);
    let mut m = machine();
    let loaded = m.load(&program, &Linked::default()).unwrap();
    assert_eq!(m.load(&program, &Linked::default()), Err(LoadError::Loaded));
    // 断った後も、読み込んだプログラムはそのまま動く
    assert_eq!(m.run(loaded, checked()).unwrap().exit, None);
    assert_eq!(
        m.call("f", &[Arg::I64(4)], checked()).unwrap(),
        [Arg::I64(4)]
    );
    m.close();

    // 失敗した読み込みも、マシンの1回の読み込みである
    let mut m = machine();
    let refused = crate::common::native::program("fn entry$main() -> float {\n  return 1.5\n}\n");
    assert!(matches!(
        m.load(&refused, &Linked::default()),
        Err(LoadError::Entry { .. })
    ));
    assert_eq!(m.load(&program, &Linked::default()), Err(LoadError::Loaded));
    m.close();
}

#[test]
fn symbols_cover_every_function() {
    let text = format!(
        "{MAIN}\
         fn f(x.0: int) -> int {{\n  return x.0\n}}\n\
         fn g(x.0: float, n.1: int) -> (float, int) {{\n  return (x.0, n.1)\n}}\n\
         fn h() -> unit {{\n  return ()\n}}\n"
    );
    let program = program(&text);
    let names = |m: &eml_native::NativeMachine| {
        let mut names: Vec<String> = m.symbols().iter().map(|s| s.name.clone()).collect();
        names.sort();
        names
    };
    let mut m = machine();
    let loaded = m.load(&program, &Linked::default()).unwrap();
    // runと`call`が作るthunkも記号を持つ
    m.run(loaded, checked()).unwrap();
    m.call("f", &[Arg::I64(1)], checked()).unwrap();

    let image = m.image().unwrap();
    // SAFETY: マシンが生きている間、`Image`の表は生きていて変わらない
    let rows = unsafe { image.functions.as_slice() };
    assert_eq!(rows.len(), program.functions.len());
    let symbols = m.symbols();
    for row in rows {
        let code = row.code.addr();
        // SAFETY: 上と同じ
        let name = unsafe { row.name.as_str() };
        let holder = symbols
            .iter()
            .find(|s| s.address.addr() <= code && code < s.address.addr() + s.size as usize)
            .unwrap_or_else(|| panic!("no symbol holds `{name}`"));
        assert_eq!(holder.name, format!("f:{name}"));
        assert_eq!(holder.size, row.size);
    }
    let mut ranges: Vec<(usize, usize)> = symbols
        .iter()
        .map(|s| (s.address.addr(), s.address.addr() + s.size as usize))
        .collect();
    ranges.sort();
    for pair in ranges.windows(2) {
        assert!(pair[0].1 <= pair[1].0, "symbols overlap: {pair:?}");
    }
    assert!(symbols.iter().all(|s| s.size > 0));

    let mut fresh = machine();
    let loaded = fresh.load(&program, &Linked::default()).unwrap();
    fresh.run(loaded, checked()).unwrap();
    fresh.call("f", &[Arg::I64(1)], checked()).unwrap();
    assert_eq!(names(&m), names(&fresh));
    m.close();
    fresh.close();
}

#[test]
fn a_function_without_calls_agrees_with_the_vm() {
    agree("fn entry$main() -> unit {\n  return ()\n}\n");
}

#[test]
fn a_lowered_function_without_calls_agrees_with_the_vm() {
    agree_lowered("fn entry$main() -> unit {\n  return ()\n}\n");
}
