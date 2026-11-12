//! 直接の呼び出し、複数の値を返す呼び出し、末尾呼び出しと、呼び出しの後の巻き戻しの確かめ
//! (docs/spec/native.md の「関数の形」、docs/spec/native.md の「巻き戻しの地点」)。JITしたコードにエントリのstubを通して
//! 入るので、Miriでは流さない。
#![cfg(not(miri))]
use eml_native::Arg;

use crate::common::native::{agree, call, clif, native};

const MAIN: &str = "fn entry$main() -> unit {\n  return ()\n}\n";

#[test]
fn a_direct_call_returns_its_result() {
    let text = "\
fn show(n.0: int) -> obj {
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  return s.1
}
fn entry$main() -> unit {
  let s.0: obj = call show(7)
  let u.1: unit = extern Prelude.println(s.0)
  let t.2: obj = call show(-12)
  let v.3: unit = extern Prelude.println(t.2)
  return v.3
}
";
    agree(text);
    assert_eq!(native(text), ("7\n-12\n".to_string(), Ok(None)));
    let text = format!(
        "{MAIN}\
         fn id(x.0: int) -> int {{\n  return x.0\n}}\n\
         fn f(x.0: int) -> int {{\n  let y.1: int = call id(x.0)\n  let z.2: int = call id(y.1)\n  return z.2\n}}\n"
    );
    for n in [0, -3, i64::MAX] {
        assert_eq!(call(&text, "f", &[Arg::I64(n)]), [Arg::I64(n)]);
    }
}

#[test]
fn many_values_come_back() {
    // 呼んだ側は、呼び出しの結果の葉をすべて、値の順に束縛する
    let text = "\
fn two(a.0: int, b.1: int) -> (obj, int, obj) {
  let s.2: obj = extern \"Prelude.Show Int.show\"(a.0)
  let t.3: obj = extern \"Prelude.Show Int.show\"(b.1)
  return (t.3, a.0, s.2)
}
fn entry$main() -> unit {
  let (x.0: obj, n.1: int, y.2: obj) = call two(1, 2)
  let u.3: unit = extern Prelude.println(x.0)
  let z.4: obj = extern \"Prelude.Show Int.show\"(n.1)
  let v.5: unit = extern Prelude.println(z.4)
  let w.6: unit = extern Prelude.println(y.2)
  return w.6
}
";
    agree(text);
    assert_eq!(native(text), ("2\n1\n1\n".to_string(), Ok(None)));
    let text = format!(
        "{MAIN}\
         fn g(a.0: int) -> (enum, int, tobj) {{\n  return (#2, a.0, #3)\n}}\n\
         fn f(a.0: int) -> (int, tobj, enum) {{\n  \
         let (e.1: enum, b.2: int, t.3: tobj) = call g(a.0)\n  return (b.2, t.3, e.1)\n}}\n"
    );
    assert_eq!(
        call(&text, "f", &[Arg::I64(9)]),
        [Arg::I64(9), Arg::I64((3 << 1) | 1), Arg::I64(2)]
    );
}

#[test]
fn a_tail_call_to_a_callee_with_more_stack_arguments() {
    // 呼ぶ先は呼ぶ側より多くの引数をスタックで受けるので、末尾呼び出しは呼ぶ側の引数の領域を広げる
    let params: Vec<String> = (0..12).map(|i| format!("p.{i}: int")).collect();
    let text = format!(
        "{MAIN}\
         fn g({}) -> (int, int, int, int) {{\n  return (p.0, p.6, p.10, p.11)\n}}\n\
         fn f(a.0: int, b.1: int) -> (int, int, int, int) {{\n  \
         tail call g(a.0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, b.1)\n}}\n",
        params.join(", ")
    );
    assert_eq!(
        call(&text, "f", &[Arg::I64(-1), Arg::I64(i64::MIN)]),
        [Arg::I64(-1), Arg::I64(6), Arg::I64(10), Arg::I64(i64::MIN)]
    );
}

#[test]
fn float_parameters_and_results() {
    let text = format!(
        "{MAIN}\
         fn g(x.0: float, n.1: int, y.2: float) -> (float, int, float) {{\n  return (y.2, n.1, x.0)\n}}\n\
         fn h(x.0: float, y.1: float) -> float {{\n  \
         let z.2: float = extern \"Prelude.Div Float.%\"(x.0, y.1)\n  return z.2\n}}\n\
         fn f(x.0: float, y.1: float) -> (float, int, float, float) {{\n  \
         let (a.2: float, n.3: int, b.4: float) = call g(x.0, 5, y.1)\n  \
         let c.5: float = call h(a.2, b.4)\n  \
         tail call k(a.2, n.3, b.4, c.5)\n}}\n\
         fn k(a.0: float, n.1: int, b.2: float, c.3: float) -> (float, int, float, float) {{\n  \
         return (a.0, n.1, b.2, c.3)\n}}\n"
    );
    assert_eq!(
        call(&text, "f", &[Arg::F64(2.0), Arg::F64(7.5)]),
        [Arg::F64(7.5), Arg::I64(5), Arg::F64(2.0), Arg::F64(1.5)]
    );
}

#[test]
fn an_exit_in_a_callee_unwinds_the_caller() {
    // 呼んだ側は表示した文字列`s.0`と`t.1`を持ち、3つ目の`m.2`を渡して呼ぶ。呼ばれた側は`m.2`を消費してから
    // `exit`する。呼び出しの後の確かめが後始末へ進み、`s.0`と`t.1`だけを手放す。漏れても二重に手放しても
    // censusかヒープが誤りにする
    let text = "\
fn stop(m.0: obj) -> unit {
  let u.1: unit = extern Prelude.println(m.0)
  let z.2: unit = extern Prelude.exit(5)
  return z.2
}
fn entry$main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(1)
  let t.1: obj = extern \"Prelude.Show Int.show\"(2)
  let m.2: obj = extern \"Prelude.Show Int.show\"(3)
  let z.3: unit = call stop(m.2)
  let u.4: unit = extern Prelude.println(s.0)
  let v.5: unit = extern Prelude.println(t.1)
  return v.5
}
";
    agree(text);
    assert_eq!(native(text), ("3\n".to_string(), Ok(Some(5))));
}

#[test]
fn an_exit_in_a_callee_releases_a_value_held_twice() {
    // 呼んだ側は`s.0`を参照2つで持って呼ぶ。呼び出しの後の後始末はそれを2回手放す。1回足りなければcensusが漏れを
    // 見つける。1回多ければ、速いモードのネイティブでは解放した物体の使用で、ヒープは誤りにしない。`agree`の検査モードの
    // VMのrunが確かめるのは、LIRの参照の釣り合いである
    let text = "\
fn stop(m.0: obj) -> unit {
  let u.1: unit = extern Prelude.println(m.0)
  let z.2: unit = extern Prelude.exit(5)
  return z.2
}
fn entry$main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(1)
  dup s.0
  let m.1: obj = extern \"Prelude.Show Int.show\"(3)
  let z.2: unit = call stop(m.1)
  let u.3: unit = extern Prelude.println(s.0)
  let v.4: unit = extern Prelude.println(s.0)
  return v.4
}
";
    agree(text);
    assert_eq!(native(text), ("3\n".to_string(), Ok(Some(5))));
}

#[test]
fn an_exit_in_a_callee_that_borrows_an_argument() {
    // `look`は`s.0`を借りるだけなので、呼んだ側は呼び出しの後も`s.0`を持ち、後始末がそれを手放す
    let text = "\
fn look(borrowed s.0: obj) -> unit {
  let q.1: obj = extern \"Prelude.Show String.show\"(s.0)
  let u.2: unit = extern Prelude.println(q.1)
  let z.3: unit = extern Prelude.exit(7)
  return z.3
}
fn entry$main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(4)
  let z.1: unit = call look(s.0)
  let u.2: unit = extern Prelude.println(s.0)
  return u.2
}
";
    agree(text);
    assert_eq!(native(text), ("\"4\"\n".to_string(), Ok(Some(7))));
}

#[test]
fn an_exit_below_a_tail_call() {
    // `middle`は`stop`を末尾で呼ぶので、その後に確かめはない。`stop`が戻ると、`middle`の呼び手の`entry$main`が
    // 中断を見て、自分の持つ`s.0`を手放す
    let text = "\
fn stop(n.0: int) -> unit {
  let z.1: unit = extern Prelude.exit(n.0)
  return z.1
}
fn middle(n.0: int) -> unit {
  tail call stop(n.0)
}
fn entry$main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(1)
  let z.1: unit = call middle(8)
  let u.2: unit = extern Prelude.println(s.0)
  return u.2
}
";
    agree(text);
    assert_eq!(native(text), (String::new(), Ok(Some(8))));
}

#[test]
fn an_exit_two_frames_down() {
    // 中断は2つのフレームを抜け、どちらのフレームも自分の持つ文字列を手放す
    let text = "\
fn stop(n.0: int) -> unit {
  let z.1: unit = extern Prelude.exit(n.0)
  return z.1
}
fn middle(n.0: int) -> unit {
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let z.2: unit = call stop(n.0)
  let u.3: unit = extern Prelude.println(s.1)
  return u.3
}
fn entry$main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(1)
  let u.1: unit = extern Prelude.println(s.0)
  let t.2: obj = extern \"Prelude.Show Int.show\"(2)
  let z.3: unit = call middle(6)
  let v.4: unit = extern Prelude.println(t.2)
  return v.4
}
";
    agree(text);
    assert_eq!(native(text), ("1\n".to_string(), Ok(Some(6))));
}

/// 直接の呼び出しを持つ関数。
const CALLER: &str = "\
fn f(s.0: obj, n.1: int) -> unit {
  let m.2: int = call g(n.1)
  tail call h(s.0, m.2)
}
fn g(n.0: int) -> int {
  return n.0
}
fn h(s.0: obj, n.1: int) -> unit {
  let u.2: unit = extern Prelude.println(s.0)
  return u.2
}
";

#[test]
fn a_function_with_calls_has_the_same_code_in_two_programs() {
    // 2つ目のプログラムでは、呼ぶ先の宣言の番号がほかの関数10個の分だけずれる。コードは呼ぶ先を記号で名指すので、
    // CLIFの文は変わらない
    let alone = format!("{MAIN}{CALLER}");
    let others: String = (0..10)
        .map(|i| {
            format!("fn k{i}(n.0: int) -> int {{\n  let m.1: int = call g(n.0)\n  return m.1\n}}\n")
        })
        .collect();
    let after = format!("{MAIN}{others}{CALLER}");
    assert_eq!(clif(&alone, "f"), clif(&after, "f"));
}
