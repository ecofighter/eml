//! 型どおりの `apply` と、実行時の変換 (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「VM」)。
//! 関数は一様でなく、`apply` の場所は引数と結果の Repr を持つ。速い道では箱を作らず、呼ぶ所と関数の Repr が合わない
//! 引数だけを `box` か `unbox` で変える。捕まえた値と部分適用の引数は関数の引数の Repr のまま持つ。

use crate::common::{run_typed_apply, run_unverified};

const SHOW_INT: &str = "\
fn show(n.0: int) -> unit {
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
";

const SHOW_FLOAT: &str = "\
fn show(x.0: float) -> unit {
  let s.1: obj = extern \"Prelude.Show Float.show\"(x.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
";

const INC: &str = "\
fn inc(x.0: int) -> int {
  let y.1: int = extern \"Prelude.Num Int.+\"(x.0, 1)
  return y.1
}
";

/// 呼ぶ所と関数の種類が合えば、箱を作らずに入る。
#[test]
fn a_typed_apply_takes_the_fast_path() {
    let text = format!(
        "\
fn main() -> unit {{
  let r.0: int = apply &inc(10: int)
  tail call show(r.0)
}}
{INC}{SHOW_INT}"
    );
    let (out, stats) = run_typed_apply(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "11\n");
    assert_eq!((stats.conversions, stats.boxes), (0, 0), "{stats:?}");
}

/// 一様な場所は引数を `tobj` で渡すので、`int` を受ける関数の前で `unbox` する。箱は消費する。
#[test]
fn a_uniform_site_boxes_into_a_typed_function() {
    let text = format!(
        "\
fn main() -> unit {{
  let b.0: tobj = box 10
  let r.1: int = uniform apply &inc(b.0)
  tail call show(r.1)
}}
{INC}{SHOW_INT}"
    );
    let (out, stats) = run_typed_apply(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "11\n");
    assert_eq!((stats.conversions, stats.unboxes), (1, 1), "{stats:?}");
}

/// 捕まえた `float` は生の語のまま持つので、箱を作らない。
#[test]
fn a_closure_keeps_a_float_capture_raw() {
    let text = format!(
        "\
fn main() -> unit {{
  let c.0: tobj = closure scale(2.5)
  let r.1: float = apply c.0(4.0: float)
  tail call show(r.1)
}}
fn scale(k.0: float, x.1: float) -> float {{
  let y.2: float = extern \"Prelude.Num Float.*\"(k.0, x.1)
  return y.2
}}
{SHOW_FLOAT}"
    );
    let (out, stats) = run_typed_apply(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "10.0\n");
    assert_eq!(stats.boxes, 0, "{stats:?}");
}

/// 引数の足りない `apply` で VM が作るクロージャも、関数の引数の Repr で並べる。参照の引数は走査する語に、`int` は
/// 生の語に入る。
#[test]
fn a_partial_application_by_the_vm_lays_out_raw_words() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"sum \"
  let c.1: tobj = apply &add3(s.0, 1: int)
  let d.2: tobj = apply c.1(2: int)
  let r.3: obj = apply d.2(3: int)
  let u.4: unit = extern Prelude.println(r.3)
  return u.4
}
fn add3(s.0: obj, a.1: int, b.2: int, c.3: int) -> obj {
  let t.4: int = extern \"Prelude.Num Int.+\"(a.1, b.2)
  let n.5: int = extern \"Prelude.Num Int.+\"(t.4, c.3)
  let m.6: obj = extern \"Prelude.Show Int.show\"(n.5)
  let r.7: obj = extern \"Prelude.++\"(s.0, m.6)
  return r.7
}
";
    let (out, stats) = run_typed_apply(text);
    let stats = stats.unwrap();
    assert_eq!(out, "sum 6\n");
    assert_eq!((stats.boxes, stats.conversions), (0, 0), "{stats:?}");
}

/// `make` が返すクロージャ。`int` の引数を生の語で捕まえる。
const MAKE: &str = "\
fn make(x.0: int) -> tobj {
  let c.1: tobj = closure add(x.0)
  return c.1
}
fn add(a.0: int, s.1: obj, b.2: int) -> unit {
  let n.3: int = extern \"Prelude.Num Int.+\"(a.0, b.2)
  let m.4: obj = extern \"Prelude.Show Int.show\"(n.3)
  let r.5: obj = extern \"Prelude.++\"(s.1, m.4)
  let u.6: unit = extern Prelude.println(r.5)
  return u.6
}
";

/// 1つの `apply` が `make` と、`make` の返すクロージャの両方に `int` を渡す。余る引数は `ApplyRest` のフレームに
/// 場所の Repr のまま預け、後の適用は場所のシグネチャで種類を比べる。`leave` の中で `exit` すると、巻き戻しはそのフレームの
/// 参照の引数だけを手放す。生の語を `decref` すると、ビット列をポインタとして読んでしまう。`verifier` の拒む関数は巻き戻しの
/// 表を持たないので、`main` は末尾の `apply` で自分のフレームを `ApplyRest` のフレームに替える。
#[test]
fn apply_rest_keeps_raw_words() {
    let text = format!(
        "\
fn main() -> unit {{
  let s.0: obj = const \"sum \"
  let u.1: unit = apply &make(1: int, s.0, 2: int)
  return u.1
}}
{MAKE}"
    );
    let (out, stats) = run_typed_apply(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "sum 3\n");
    assert_eq!((stats.boxes, stats.conversions), (0, 0), "{stats:?}");

    let exiting = format!(
        "\
fn main() -> unit {{
  let s.0: obj = const \"sum \"
  tail apply &leave(1: int, s.0, 2: int)
}}
fn leave(x.0: int) -> tobj {{
  let z.1: unit = extern Prelude.exit(3)
  let c.2: tobj = closure add(x.0)
  return c.2
}}
{MAKE}"
    );
    let (out, stats) = run_typed_apply(&exiting);
    let stats = stats.unwrap();
    assert_eq!(out, "");
    // `leave` と `ApplyRest` の枠の2つ
    assert_eq!(stats.unwound_frames, 2, "{stats:?}");
}

/// クロージャの最初の語の即値は関数の番号と捕まえた値の数を持つ。`con` で作った物体の最初の語は `&first` (数は 0)
/// なのに、物体はもう1つ走査する語を持つので、検査モードは食い違いを断る。関数でない値を適用するのは型の誤りで、
/// verifier は見ないので、通さずに実行する。
#[test]
fn the_capture_count_is_packed_with_the_function() {
    let text = "\
layout Pair { Pair(tobj, tobj) }
fn main() -> unit {
  let b.0: tobj = box 1
  let p.1: tobj = con Pair #0(&first, b.0)
  let c.2: tobj = box 2
  let r.3: tobj = apply p.1(c.2)
  decref r.3
  return ()
}
fn first(a.0: tobj, b.1: tobj) -> tobj {
  decref b.1
  return a.0
}
";
    let (_, result) = run_unverified(text);
    assert_eq!(
        result.unwrap_err().to_string(),
        "internal error: the capture count packed with a function does not match its closure in `main`"
    );
}

/// 合わない組は同じ型の一様な Repr と箱を要するスカラーの組だけである。`int` の場所で `float` の関数を呼ぶのは
/// 型の誤りで、verifier は型を見ないので、通さずに実行する。
#[test]
fn a_mismatch_between_different_types_is_internal() {
    let text = format!(
        "\
fn main() -> unit {{
  let r.0: float = apply &half(4: int)
  tail call show(r.0)
}}
fn half(x.0: float) -> float {{
  let y.1: float = extern \"Prelude.Div Float./\"(x.0, 2.0)
  return y.1
}}
{SHOW_FLOAT}"
    );
    let (_, result) = run_unverified(&text);
    assert_eq!(
        result.unwrap_err().to_string(),
        "internal error: an apply converts between the reprs of different types in `main`"
    );
}

/// 引数が 15 個を超えるシグネチャは語に詰められないので、合っていても種類を中身で比べる。変換はしない。
#[test]
fn a_signature_too_long_to_pack_is_compared_by_its_reprs() {
    let n = 20;
    let params: Vec<String> = (0..n).map(|i| format!("a.{i}: int")).collect();
    let captured: Vec<String> = (0..n - 1).map(|i| format!("{i}")).collect();
    let mut text = format!(
        "\
fn main() -> unit {{
  let c.0: tobj = closure sum({})
  let r.1: int = apply c.0(100: int)
  tail call show(r.1)
}}
fn sum({}) -> int {{
",
        captured.join(", "),
        params.join(", ")
    );
    text += &format!("  let s.{n}: int = extern \"Prelude.Num Int.+\"(a.0, a.1)\n");
    for i in 2..n {
        text += &format!(
            "  let s.{}: int = extern \"Prelude.Num Int.+\"(s.{}, a.{i})\n",
            n + i - 1,
            n + i - 2
        );
    }
    text += &format!("  return s.{}\n}}\n{SHOW_INT}", 2 * n - 2);
    let (out, stats) = run_typed_apply(&text);
    let stats = stats.unwrap();
    // 0 から 18 までの和と 100
    assert_eq!(out, "271\n");
    assert_eq!((stats.conversions, stats.boxes), (0, 0), "{stats:?}");
}
