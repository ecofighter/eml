//! `exit`と`panic`の行が始める中断と、その地点の後始末 (docs/spec/native.md の「巻き戻しの地点」)。JITしたコードに
//! エントリのstubを通して入るので、Miriでは流さない。
#![cfg(not(miri))]
use eml_lir::{Rhs, Stmt};

use crate::common::native::{agree, native, program};

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

/// `program`の関数`name`の、ただ1つの直接の呼び出しの巻き戻しの地点。
fn call_point(program: &eml_lir::Program, name: &str) -> eml_lir::unwind::UnwindPoint {
    let index = program
        .functions
        .iter()
        .position(|function| function.name == name)
        .unwrap_or_else(|| panic!("no function `{name}`"));
    let function = &program.functions[index];
    let points = eml_lir::unwind::points(program, |_| true);
    let unwinds = points[index]
        .as_ref()
        .expect("every function is wanted")
        .as_ref()
        .unwrap_or_else(|error| panic!("{error}"));
    let calls: Vec<_> = unwinds
        .points
        .iter()
        .filter(|point| {
            matches!(
                function.blocks[point.block.0 as usize].stmts[point.stmt],
                Stmt::Let {
                    rhs: Rhs::Call(_),
                    ..
                }
            )
        })
        .collect();
    let [point] = calls[..] else {
        panic!("`{name}` has {} call points", calls.len())
    };
    point.clone()
}

#[test]
fn exit_unwinds_two_frames_holding_owned_values_and_a_token() {
    // `g`の`exit`は`f`と`main`の地点の後始末を通る。`f`は`call g`をまたいで文字列2つと生きたtokenを持ち、`main`は
    // `call f`をまたいで文字列を持つ。後始末が参照かtokenの箱を残せば、censusが漏れを見つける
    let text = "\
layout P { P(obj, int) }
fn g() -> unit {
  let z.0: unit = extern Prelude.exit(3)
  return z.0
}
fn f(x.0: obj) -> obj {
  unpack x.0 P #0(s.1: obj, n.2: int)
  let t.3: obj = extern \"Prelude.Show Int.show\"(n.2)
  let r.4: token = release x.0 P #0(s.1, _)
  let u.5: unit = call g()
  decref s.1
  let d.6: obj = con P #0(t.3, n.2) reuse r.4 in place
  return d.6
}
fn entry$main() -> unit {
  let a.0: obj = extern \"Prelude.Show Int.show\"(1)
  let p.1: obj = con P #0(a.0, 2)
  let k.2: obj = extern \"Prelude.Show Int.show\"(7)
  dup k.2
  let u.3: unit = extern Prelude.println(k.2)
  let q.4: obj = call f(p.1)
  decref q.4
  decref k.2
  return u.3
}
";
    let program = program(text);
    let f = call_point(&program, "f");
    assert!(!f.owned.is_empty() && !f.tokens.is_empty(), "{f:?}");
    let main = call_point(&program, "entry$main");
    assert!(!main.owned.is_empty(), "{main:?}");
    agree(text);
    assert_eq!(native(text), ("7\n".to_string(), Ok(Some(3))));
}

/// runを致命的な誤りで終えるテスト。`eml_fatal`のlongjmpが飛び越えたフレームにはAddressSanitizerのスタックの印が
/// 残るので、そのジョブは`fatal::`を飛ばす (docs/implementation/testing.md の「`eml_native` の検証」)。
mod fatal {
    use crate::common::native::{agree, native};

    #[test]
    fn an_uncaught_panic_reports_the_message() {
        let text = "\
fn entry$main() -> unit {
  let m.0: obj = const \"boom\"
  let r.1: unit = extern Prelude.panic(m.0) @\"main.em\":3:5
  return r.1
}
";
        agree(text);
        let (out, result) = native(text);
        assert_eq!(out, "");
        insta::assert_snapshot!(result.unwrap_err(), @"
        panicked: boom
          at main.em:3:5
        ");
    }
}
