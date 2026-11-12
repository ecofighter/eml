//! `exit`と`panic`の行が始める中断と、その地点の後始末 (docs/spec/native.md の「巻き戻しの地点」)。JITしたコードに
//! エントリのstubを通して入るので、Miriでは流さない。
#![cfg(not(miri))]
use crate::common::native::{agree, native};

#[test]
fn exit_in_the_entry_gives_its_code() {
    let text = "\
fn entry$main() -> unit {
  let z.0: unit = extern Prelude.exit(3)
  return z.0
}
";
    agree(text);
    assert_eq!(native(text), (String::new(), Ok(Some(3))));
}

#[test]
fn exit_drops_what_the_frame_owns() {
    // `s.1`は`exit`をまたいで持つ文字列で、地点の後始末が手放す。漏れればcensusが誤りにする
    let text = "\
fn entry$main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(5)
  let t.1: obj = extern \"Prelude.Show Int.show\"(6)
  let u.2: unit = extern Prelude.println(s.0)
  let z.3: unit = extern Prelude.exit(4)
  let v.4: unit = extern Prelude.println(t.1)
  return v.4
}
";
    agree(text);
    assert_eq!(native(text), ("5\n".to_string(), Ok(Some(4))));
}

/// runを致命的な誤りで終えるテスト。`eml_fatal`のlongjmpが飛び越えたフレームにはAddressSanitizerのスタックの印が
/// 残るので、そのジョブは`fatal::`を飛ばす (docs/implementation/testing.md の「`eml_native` の検証」)。
mod fatal {
    use crate::common::native::{agree, native};

    #[test]
    fn an_uncaught_panic_reports_the_message() {
        let text = "\
fn entry$main() -> unit {
  let m.0: obj = extern \"Prelude.Show Int.show\"(42)
  let r.1: unit = extern Prelude.panic(m.0) @\"main.em\":3:5
  return r.1
}
";
        agree(text);
        let (out, result) = native(text);
        assert_eq!(out, "");
        insta::assert_snapshot!(result.unwrap_err(), @"
        panicked: 42
          at main.em:3:5
        ");
    }
}
