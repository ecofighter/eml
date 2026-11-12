//! 呼ぶ行の下ろしと、行の失敗の道 (docs/spec/native.md の「externの失敗」、docs/spec/native.md の「巻き戻しの地点」)。
//! JITしたコードにエントリのstubを通して入るので、Miriでは流さない。
#![cfg(not(miri))]
use crate::common::native::{agree, clif};

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

/// runを致命的な誤りで終えるテスト。`eml_fatal`のlongjmpが飛び越えたフレームにはAddressSanitizerのスタックの印が
/// 残るので、そのジョブは`fatal::`を飛ばす (docs/implementation/testing.md の「`eml_native` の検証」)。
mod fatal {
    use std::sync::Arc;

    use eml_nrt::HostRef;
    use eml_rt::{Buffering, StdHost};

    use crate::common::native::agree_on;

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
