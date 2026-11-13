//! JITのホスト`NativeMachine`と、呼び出しのない関数の下ろしと、入口の安全点 (docs/spec/native.md の「関数の形」、
//! docs/spec/native.md の「安全点」)。JITしたコードにエントリのstubを通して入るので、Miriでは流さない。
#![cfg(not(miri))]
use std::sync::Arc;

use eml_cabi::Linked;
use eml_native::{Arg, LoadError, NativeMachine};
use eml_nrt::RunConfig;
use eml_nrt::image::Options;
use eml_rt::{Control, Limits};
use eml_test_support::capture;

use crate::common::native::{agree, agree_lowered, call, clif, machine, native_options, program};

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
    assert_eq!(m.run(loaded, native_options()).unwrap().exit, None);
    assert_eq!(
        m.call("f", &[Arg::I64(4)], native_options()).unwrap(),
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
    m.run(loaded, native_options()).unwrap();
    m.call("f", &[Arg::I64(1)], native_options()).unwrap();

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
    fresh.run(loaded, native_options()).unwrap();
    fresh.call("f", &[Arg::I64(1)], native_options()).unwrap();
    assert_eq!(names(&m), names(&fresh));
    m.close();
    fresh.close();
}

/// 関数の値のエントリと`apply`の補助の関数も記号を持つ。`main`は`&make`を3つの引数に適用するので、moduleは
/// `eml$apply$1`から`eml$apply$3`を定義する。値として使う関数`make`と`add`は、それぞれ`c0`のエントリを持つ。
#[test]
fn symbols_cover_adapters_and_helpers() {
    let text = "\
fn make(x.0: tobj) -> tobj {
  decref x.0
  return &add
}
fn add(s.0: obj, b.1: tobj) -> unit {
  decref s.0
  decref b.1
  return ()
}
fn entry$main() -> unit {
  let s.0: obj = const \"s\"
  let a.1: tobj = box 1
  let b.2: tobj = box 2
  dup s.0
  let u.3: unit = apply &make(a.1, s.0, b.2)
  let c.4: tobj = closure add(s.0)
  let d.5: tobj = box 3
  let v.6: unit = apply c.4(d.5)
  return v.6
}
";
    // プログラムの関数、thunk、`eml$raise`のほかに定義する記号。後のエントリと補助の関数も、ここに名前を足す
    let adapters = [(0, "make"), (0, "add"), (1, "add")];
    let helpers = ["eml$apply$1", "eml$apply$2", "eml$apply$3"];
    let mut expected: Vec<String> = adapters
        .iter()
        .map(|(captures, name)| format!("c{captures}:{name}"))
        .chain(helpers.iter().map(|name| name.to_string()))
        .collect();
    expected.sort();

    let mut m = machine();
    m.load(&program(text), &Linked::default()).unwrap();
    let symbols = m.symbols();
    let mut others: Vec<String> = symbols
        .iter()
        .map(|s| s.name.clone())
        .filter(|name| !name.starts_with("f:") && name != "eml$raise")
        .collect();
    others.sort();
    assert_eq!(others, expected);
    assert!(symbols.iter().all(|s| s.size > 0));
    let mut ranges: Vec<(usize, usize)> = symbols
        .iter()
        .map(|s| (s.address.addr(), s.address.addr() + s.size as usize))
        .collect();
    ranges.sort();
    for pair in ranges.windows(2) {
        assert!(pair[0].1 <= pair[1].0, "symbols overlap: {pair:?}");
    }
    // 静的なクロージャのコードの語は、その関数の`c0`のエントリの始めである
    let image = m.image().unwrap();
    // SAFETY: マシンが生きている間、`Image`の表は生きていて変わらない
    let (rows, data) = unsafe { (image.functions.as_slice(), image.data.as_slice()) };
    let valued: Vec<_> = rows.iter().filter(|row| !row.value.is_null()).collect();
    assert_eq!(
        valued.len(),
        adapters
            .iter()
            .filter(|(captures, _)| *captures == 0)
            .count()
    );
    for row in valued {
        // SAFETY: 上と同じ
        let name = unsafe { row.name.as_str() };
        let datum = data
            .iter()
            .find(|datum| datum.slot == row.value)
            .unwrap_or_else(|| panic!("no datum for `{name}`"));
        let adapter = symbols
            .iter()
            .find(|s| s.name == format!("c0:{name}"))
            .unwrap_or_else(|| panic!("no adapter for `{name}`"));
        assert_eq!(datum.code, adapter.address, "{name}");
    }
    m.close();
}

#[test]
fn a_function_without_calls_agrees_with_the_vm() {
    agree("fn entry$main() -> unit {\n  return ()\n}\n");
}

#[test]
fn a_lowered_function_without_calls_agrees_with_the_vm() {
    agree_lowered("fn entry$main() -> unit {\n  return ()\n}\n");
}

/// `text`のプログラムを`config`と`options`で流し、標準出力と、終了コードか誤りの文を返す。
fn run_with(
    text: &str,
    config: &RunConfig,
    options: Options,
) -> (String, Result<Option<i64>, String>) {
    let host = capture();
    let out = host.output();
    let mut machine = NativeMachine::new(config, host.into_ref());
    let loaded = machine
        .load(&program(text), &Linked::default())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let finished = machine
        .run(loaded, options)
        .map(|finished| finished.exit)
        .map_err(|error| error.to_string());
    machine.close();
    (out.contents(), finished)
}

#[test]
fn a_notice_does_not_stop_the_run() {
    // 柔らかい上限が1バイトなので、ヒープが最初にページを取ると、workerの語に知らせが立つ。次の入口の安全点は知らせを
    // 受け取ってrunを続ける
    let text = "\
fn show(n.0: int) -> unit {
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
fn entry$main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(1)
  let u.1: unit = extern Prelude.println(s.0)
  let v.2: unit = call show(2)
  return v.2
}
";
    let control = Arc::new(Control::new(Limits {
        soft_bytes: 1,
        ..Limits::default()
    }));
    let config = RunConfig::default().with_control(Arc::clone(&control));
    assert_eq!(
        run_with(text, &config, native_options()),
        ("1\n2\n".to_string(), Ok(None))
    );
    assert!(control.taken(0) > 1, "the heap took pages during the run");
    assert_eq!(
        control.worker_bits(0) & Control::NOTICE,
        0,
        "a safepoint took the notice"
    );
}

/// runを致命的な誤りで終えるテスト。`eml_fatal`のlongjmpが飛び越えたフレームにはAddressSanitizerのスタックの印が
/// 残るので、そのジョブは`fatal::`を飛ばす (docs/implementation/testing.md の「`eml_native` の検証」)。
mod fatal {
    use eml_nrt::RunConfig;
    use eml_nrt::image::Options;

    use super::run_with;
    use crate::common::native::native_options;

    #[test]
    fn deep_recursion_hits_the_stack_limit() {
        // 末尾でない自分への呼び出しは、workerのスタックを使い切るまで深くなる。深さの上限はスタックなので、64 MiBの
        // スタックではおよそ10^6のフレームで止まる
        let text = "\
fn f(n.0: int) -> int {
  let r.1: int = call f(n.0)
  return r.1
}
fn entry$main() -> unit {
  let r.0: int = call f(0)
  return ()
}
";
        let options = Options {
            stack: 64 << 20,
            ..native_options()
        };
        assert_eq!(
            run_with(text, &RunConfig::default(), options),
            (
                String::new(),
                Err("the call depth limit is exceeded in `f`".to_string())
            )
        );
    }

    #[test]
    fn a_stop_bit_stops_at_the_next_entry() {
        // 止めるビットはrunの始めでも下りないので、入口の関数の安全点が、何かを表示する前にrunを止める
        let text = "\
fn entry$main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(1)
  let u.1: unit = extern Prelude.println(s.0)
  return u.1
}
";
        let config = RunConfig::default();
        config.control.interrupt();
        assert_eq!(
            run_with(text, &config, native_options()),
            (
                String::new(),
                Err("interrupted in `entry$main`".to_string())
            )
        );
    }
}
