//! 呼ぶ行の下ろしと、行の失敗の道 (docs/spec/native.md の「externの失敗」、docs/spec/native.md の「巻き戻しの地点」)。
//! JITしたコードにエントリのstubを通して入るので、Miriでは流さない。
#![cfg(not(miri))]
use eml_extern::{Extern, OpRet};
use eml_lir::Repr;
use eml_native::Arg;
use eml_native::rows::INLINE;
use eml_nrt::RunFault;
use eml_nrt::shared_rows as shared;
use eml_rt::{Control, Ctx, Fast, Limits};

use crate::common::native::{agree, clif, host, machine, native_options, program};

const MAIN: &str = "fn entry$main() -> unit {\n  return ()\n}\n";

#[test]
fn println_of_a_shown_int() {
    agree(
        "\
fn entry$main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(42)
  let u.1: unit = extern Prelude.println(s.0) @\"main.em\":1:1
  let t.2: obj = extern \"Prelude.Show Int.show\"(-9223372036854775808)
  let v.3: unit = extern Prelude.eprintln(t.2) @\"main.em\":2:1
  let a.4: obj = extern \"Prelude.Show Int.show\"(-7)
  let b.5: obj = extern \"Prelude.Show Int.show\"(8)
  let c.6: obj = extern Prelude.++(a.4, b.5)
  let w.7: unit = extern Prelude.println(c.6) @\"main.em\":3:1
  return w.7
}
",
    );
}

#[test]
fn float_show_and_float_mod() {
    agree(
        "\
fn entry$main() -> unit {
  let x.0: float = extern \"Prelude.Div Float.%\"(7.5, 2.0)
  let s.1: obj = extern \"Prelude.Show Float.show\"(x.0)
  let u.2: unit = extern Prelude.println(s.1)
  let y.3: float = extern \"Prelude.Div Float.%\"(-7.5, 2.0)
  let t.4: obj = extern \"Prelude.Show Float.show\"(y.3)
  let v.5: unit = extern Prelude.println(t.4)
  let z.6: float = extern \"Prelude.Div Float.%\"(1.0, 0.0)
  let r.7: obj = extern \"Prelude.Show Float.show\"(z.6)
  let w.8: unit = extern Prelude.println(r.7)
  return w.8
}
",
    );
}

#[test]
fn string_rows_borrow_their_arguments() {
    // `show`、`==`、`compare`は文字列を借りるので、呼び出しの後も同じ語をまた使える
    agree(
        "\
layout Prelude.Bool { False, True }
layout Prelude.Ordering { LT, EQ, GT }
fn entry$main() -> unit {
  let a.0: obj = extern \"Prelude.Show Int.show\"(12)
  let b.1: obj = extern \"Prelude.Show Int.show\"(3)
  let q.2: obj = extern \"Prelude.Show String.show\"(a.0)
  let u.3: unit = extern Prelude.println(q.2)
  let e.4: enum = extern \"Prelude.Eq String.==\"(a.0, b.1)
  switch e.4 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let c.5: enum = extern \"Prelude.Ord String.compare\"(a.0, b.1)
  switch c.5 Prelude.Ordering { #0 -> b3, #1 -> b4, #2 -> b5 }
b2:
  let n.6: obj = extern \"Prelude.Show Int.show\"(0)
  let v.7: unit = extern Prelude.println(n.6)
  jump b6()
b3:
  let n.8: obj = extern \"Prelude.Show Int.show\"(-1)
  let v.9: unit = extern Prelude.println(n.8)
  jump b6()
b4:
  let n.10: obj = extern \"Prelude.Show Int.show\"(2)
  let v.11: unit = extern Prelude.println(n.10)
  jump b6()
b5:
  let n.12: obj = extern \"Prelude.Show Int.show\"(1)
  let v.13: unit = extern Prelude.println(n.12)
  jump b6()
b6:
  let s.14: obj = extern Prelude.++(a.0, b.1)
  let w.15: unit = extern Prelude.println(s.14)
  return w.15
}
",
    );
}

/// 失敗しうる行と戻らない行を持つ関数。
const SAME: &str = "\
fn f(n.0: int) -> unit {
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let t.2: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.3: unit = extern Prelude.println(s.1) @\"f.em\":1:1
  let z.4: unit = extern Prelude.exit(n.0)
  let v.5: unit = extern Prelude.println(t.2) @\"f.em\":2:1
  return v.5
}
";

#[test]
fn the_same_function_has_the_same_code_in_two_programs() {
    let alone = format!("{MAIN}{SAME}");
    // ほかの関数は、それぞれ自分の地点と、別のファイルの位置を持つ
    let others: String = (0..10)
        .map(|i| {
            format!(
                "fn g{i}(n.0: int) -> unit {{\n  \
                 let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)\n  \
                 let u.2: unit = extern Prelude.println(s.1) @\"g{i}.em\":{i}:1\n  \
                 return u.2\n}}\n"
            )
        })
        .collect();
    let after = format!("{MAIN}{others}{SAME}");
    assert_eq!(clif(&alone, "f"), clif(&after, "f"));
}

const INTS: [i64; 14] = [
    0,
    1,
    -1,
    2,
    i64::MAX,
    i64::MIN,
    i64::MAX - 1,
    i64::MIN + 1,
    1 << 32,
    -(1 << 32),
    3037000500,
    1 << 31,
    -(1 << 31),
    -3037000500,
];

/// 符号の立った、中身のあるNaN。
const NEGATIVE_NAN: u64 = 0xfff0_0000_0000_1234;

fn floats() -> [f64; 10] {
    [
        0.0,
        -0.0,
        1.5,
        -2.25,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::from_bits(NEGATIVE_NAN),
        f64::MIN_POSITIVE,
        f64::MAX,
    ]
}

/// インラインの行の共有の関数。行の最初の変種の名前を持つ`eml_sched::rows`の関数で、VMの側もそれを呼ぶ。
#[derive(Clone, Copy)]
enum Shared {
    Int(unsafe fn(&mut Ctx<Fast>, i64) -> Result<i64, RunFault>),
    IntInt(unsafe fn(&mut Ctx<Fast>, i64, i64) -> Result<i64, RunFault>),
    Float(unsafe fn(&mut Ctx<Fast>, f64) -> Result<f64, RunFault>),
    FloatFloat(unsafe fn(&mut Ctx<Fast>, f64, f64) -> Result<f64, RunFault>),
    FloatCmp(unsafe fn(&mut Ctx<Fast>, f64, f64) -> Result<i64, RunFault>),
}

/// 行の共有の関数。表にない行は`None`で、テストを落とす。
fn shared_of(ext: Extern) -> Option<Shared> {
    use Extern::*;
    use Shared::*;
    Some(match ext {
        IntAdd => IntInt(shared::IntAdd::<Fast>),
        IntSub => IntInt(shared::IntSub::<Fast>),
        IntMul => IntInt(shared::IntMul::<Fast>),
        IntDiv | ByteDiv => IntInt(shared::IntDiv::<Fast>),
        IntMod | ByteMod => IntInt(shared::IntMod::<Fast>),
        IntNeg => Int(shared::IntNeg::<Fast>),
        IntEq | CharEq | ByteEq => IntInt(shared::IntEq::<Fast>),
        IntNe | CharNe | ByteNe => IntInt(shared::IntNe::<Fast>),
        IntLt | CharLt | ByteLt => IntInt(shared::IntLt::<Fast>),
        IntLe | CharLe | ByteLe => IntInt(shared::IntLe::<Fast>),
        IntGt | CharGt | ByteGt => IntInt(shared::IntGt::<Fast>),
        IntGe | CharGe | ByteGe => IntInt(shared::IntGe::<Fast>),
        IntCompare | CharCompare | ByteCompare => IntInt(shared::IntCompare::<Fast>),
        CharToInt | ByteToInt | CharOfByte => Int(shared::CharToInt::<Fast>),
        FloatAdd => FloatFloat(shared::FloatAdd::<Fast>),
        FloatSub => FloatFloat(shared::FloatSub::<Fast>),
        FloatMul => FloatFloat(shared::FloatMul::<Fast>),
        FloatDiv => FloatFloat(shared::FloatDiv::<Fast>),
        FloatNeg => Float(shared::FloatNeg::<Fast>),
        FloatEq => FloatCmp(shared::FloatEq::<Fast>),
        FloatNe => FloatCmp(shared::FloatNe::<Fast>),
        FloatLt => FloatCmp(shared::FloatLt::<Fast>),
        FloatLe => FloatCmp(shared::FloatLe::<Fast>),
        FloatGt => FloatCmp(shared::FloatGt::<Fast>),
        FloatGe => FloatCmp(shared::FloatGe::<Fast>),
        BoolEq => IntInt(shared::BoolEq::<Fast>),
        BoolNe => IntInt(shared::BoolNe::<Fast>),
        _ => return None,
    })
}

fn repr_text(repr: Repr) -> &'static str {
    match repr {
        Repr::Int => "int",
        Repr::Enum => "enum",
        Repr::Float => "float",
        _ => unreachable!("an inline row takes and gives `int`, `enum` and `float`"),
    }
}

/// `INLINE`の行ごとに、その行1つだけを持つ関数`r<i>`を並べたプログラム。行を1回の読み込みにまとめる。
fn one_row_functions() -> String {
    let mut text = MAIN.to_string();
    for (i, (ext, _)) in INLINE.iter().enumerate() {
        let row = ext.row();
        let params: Vec<String> = row
            .params
            .iter()
            .enumerate()
            .map(|(j, &repr)| format!("a.{j}: {}", repr_text(repr)))
            .collect();
        let args: Vec<String> = (0..row.params.len()).map(|j| format!("a.{j}")).collect();
        let OpRet::Repr(ret) = row.ret else {
            panic!("`{ext:?}` returns")
        };
        let n = row.params.len();
        text += &format!(
            "fn r{i}({}) -> {ret} {{\n  let r.{n}: {ret} = extern \"{}\"({})\n  return r.{n}\n}}\n",
            params.join(", "),
            row.name,
            args.join(", "),
            ret = repr_text(ret),
        );
    }
    text
}

/// 行の引数の組と、共有の関数の答え。
fn cases(shared: Shared) -> Vec<(Vec<Arg>, Result<Arg, RunFault>)> {
    let mut ctx = Ctx::<Fast>::new(std::sync::Arc::new(Control::new(Limits::default())), host());
    let ctx = &mut ctx;
    let mut cases = Vec::new();
    // SAFETY: インラインの行は物体を取らず、`ctx`を使わない
    unsafe {
        match shared {
            Shared::Int(f) => {
                for a in INTS {
                    cases.push((vec![Arg::I64(a)], f(ctx, a).map(Arg::I64)));
                }
            }
            Shared::IntInt(f) => {
                for a in INTS {
                    for b in INTS {
                        cases.push((vec![Arg::I64(a), Arg::I64(b)], f(ctx, a, b).map(Arg::I64)));
                    }
                }
            }
            Shared::Float(f) => {
                for a in floats() {
                    cases.push((vec![Arg::F64(a)], f(ctx, a).map(Arg::F64)));
                }
            }
            Shared::FloatFloat(f) => {
                for a in floats() {
                    for b in floats() {
                        cases.push((vec![Arg::F64(a), Arg::F64(b)], f(ctx, a, b).map(Arg::F64)));
                    }
                }
            }
            Shared::FloatCmp(f) => {
                for a in floats() {
                    for b in floats() {
                        cases.push((vec![Arg::F64(a), Arg::F64(b)], f(ctx, a, b).map(Arg::I64)));
                    }
                }
            }
        }
    }
    cases
}

/// 値のビット。`F64`は`to_bits`で比べるので、NaNの中身と`-0.0`も区別する。
fn bits(arg: Arg) -> (bool, u64) {
    match arg {
        Arg::I64(n) => (false, n as u64),
        Arg::F64(x) => (true, x.to_bits()),
    }
}

/// `INLINE`の行の引数の組1つ。
struct Case {
    ext: Extern,
    /// `one_row_functions`の、その行の関数の名前。
    name: String,
    args: Vec<Arg>,
    /// 共有の関数の答え。
    expected: Result<Arg, RunFault>,
}

fn every_case() -> Vec<Case> {
    let mut all = Vec::new();
    for (i, &(ext, _)) in INLINE.iter().enumerate() {
        let shared = shared_of(ext).unwrap_or_else(|| panic!("`{ext:?}` has no shared function"));
        for (args, expected) in cases(shared) {
            all.push(Case {
                ext,
                name: format!("r{i}"),
                args,
                expected,
            });
        }
    }
    all
}

#[test]
fn every_inline_row_agrees_with_its_shared_function() {
    // 失敗する組は`fatal::every_failing_pair_gives_the_fault_text`が確かめる
    let mut m = machine();
    m.load(&program(&one_row_functions()), &eml_cabi::Linked::default())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let mut compared = 0;
    for Case {
        ext,
        name,
        args,
        expected,
    } in every_case()
    {
        let Ok(expected) = expected else { continue };
        let got = m
            .call(&name, &args, native_options())
            .unwrap_or_else(|error| panic!("`{ext:?}` {args:?}: {error}"));
        assert_eq!(
            got.iter().copied().map(bits).collect::<Vec<_>>(),
            [bits(expected)],
            "`{ext:?}` {args:?}: {got:?}, expected {expected:?}"
        );
        compared += 1;
    }
    m.close();
    assert!(compared > 4000, "{compared}");
}

/// runを致命的な誤りで終えるテスト。`eml_fatal`のlongjmpが飛び越えたフレームにはAddressSanitizerのスタックの印が
/// 残るので、そのジョブは`fatal::`を飛ばす (docs/implementation/testing.md の「`eml_native` の検証」)。
mod fatal {
    use std::sync::Arc;

    use eml_nrt::HostRef;
    use eml_rt::{Buffering, StdHost};

    use super::{Case, every_case};
    use crate::common::native::{agree, agree_on, machine, native, native_options, program};

    #[test]
    fn every_failing_pair_gives_the_fault_text() {
        // 共有の関数が誤りを返す組では、行は算術の失敗を置いて上げる。関数`r<i>`の行は位置を持たないので、誤りは関数を
        // 名指す
        let mut m = machine();
        m.load(
            &program(&super::one_row_functions()),
            &eml_cabi::Linked::default(),
        )
        .unwrap_or_else(|error| panic!("{error:?}"));
        let mut failed = 0;
        for Case {
            ext,
            name,
            args,
            expected,
        } in every_case()
        {
            let Err(fault) = expected else { continue };
            let got = m
                .call(&name, &args, native_options())
                .map_err(|error| error.to_string());
            assert_eq!(
                got,
                Err(format!("{fault} in `{name}`")),
                "`{ext:?}` {args:?}"
            );
            failed += 1;
        }
        m.close();
        assert!(failed > 0);
    }

    #[test]
    fn an_overflow_reports_its_position() {
        let text = "\
fn entry$main() -> unit {
  let n.0: int = extern \"Prelude.Num Int.+\"(9223372036854775806, 1) @\"main.em\":2:3
  let m.1: int = extern \"Prelude.Num Int.+\"(n.0, 1) @\"main.em\":3:7
  let s.2: obj = extern \"Prelude.Show Int.show\"(m.1)
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
";
        agree(text);
        let (out, result) = native(text);
        assert_eq!(out, "");
        insta::assert_snapshot!(result.unwrap_err(), @"
        integer overflow
          at main.em:3:7
        ");
    }

    #[test]
    fn a_fault_without_a_position_names_the_function() {
        let text = "\
fn f(a.0: int, b.1: int) -> int {
  let q.2: int = extern \"Prelude.Div Int./\"(a.0, b.1)
  return q.2
}
fn entry$main() -> unit {
  let q.0: int = call f(7, 0)
  return ()
}
";
        agree(text);
        let (out, result) = native(text);
        assert_eq!(out, "");
        insta::assert_snapshot!(result.unwrap_err(), @"division by zero in `f`");
    }

    #[test]
    fn an_overflow_drops_what_the_frame_owns() {
        // `s.1`は算術の地点をまたいで持つ文字列で、地点は後始末でそれを手放す。致命的な誤りの後はcensusを流さないので、
        // 確かめるのは誤りだけである
        let text = "\
fn f(n.0: int) -> int {
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let m.2: int = extern \"Prelude.Num Int.*\"(n.0, n.0) @\"f.em\":4:9
  let u.3: unit = extern Prelude.println(s.1)
  return m.2
}
fn entry$main() -> unit {
  let t.0: obj = extern \"Prelude.Show Int.show\"(1)
  let m.1: int = call f(4294967296)
  let u.2: unit = extern Prelude.println(t.0)
  return u.2
}
";
        let program = program(text);
        let points = eml_lir::unwind::points(&program, |_| true);
        let f = points[0]
            .as_ref()
            .expect("every function is wanted")
            .as_ref()
            .unwrap_or_else(|error| panic!("{error}"));
        // 掛け算は`f`の入口のブロックの2つ目の文で、その地点は`s.1`を持つ
        let multiply = f
            .points
            .iter()
            .find(|point| point.block == eml_lir::BlockId(0) && point.stmt == 1)
            .unwrap_or_else(|| panic!("no point at the multiply: {f:?}"));
        assert_eq!(multiply.owned, [(eml_lir::VarId(1), 1)], "{multiply:?}");
        agree(text);
        let (out, result) = native(text);
        assert_eq!(out, "");
        insta::assert_snapshot!(result.unwrap_err(), @"
        integer overflow
          at f.em:4:9
        ");
    }

    /// 書けない出力。
    struct Broken;

    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }

    #[test]
    fn a_write_failure_is_fatal_with_its_text() {
        // 行ごとに書くので、`println`の中で書けないことが分かり、その地点の位置を持つ誤りになる
        let host = || {
            HostRef::new(Arc::new(StdHost::new(
                Box::new(Broken),
                Box::new(std::io::sink()),
                std::path::PathBuf::new(),
                Buffering::Line,
            )))
        };
        let result = agree_on(
            "\
fn entry$main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(1)
  let t.1: obj = extern \"Prelude.Show Int.show\"(2)
  let u.2: unit = extern Prelude.println(s.0) @\"main.em\":4:3
  let v.3: unit = extern Prelude.println(t.1) @\"main.em\":5:3
  return v.3
}
",
            host,
        );
        insta::assert_snapshot!(result.unwrap_err(), @"
        cannot write the output: broken pipe
          at main.em:4:3
        ");
    }
}
