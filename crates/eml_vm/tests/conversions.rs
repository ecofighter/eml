//! 一様な `apply` と、VM の実行時の変換の見張り (docs/spec/vm.md の「シグネチャ」)。
//! box の挿入が `apply` の引数と結果を一様にするので、変換は手で書いた、verifier を通らない LIR でだけ起きる。
//! 捕まえた値は関数の引数の Repr のまま持つ。

use crate::common::{run_lir_stats, run_unverified};

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

/// 一様な関数への `apply` は、箱を作らずに語を写して入る。`int` の引数と結果の箱は即値である。
#[test]
fn a_uniform_apply_takes_the_fast_path() {
    let text = format!(
        "\
fn main() -> unit {{
  let b.0: tobj = box 10
  let r.1: tobj = apply &inc(b.0)
  let n.2: int = unbox r.1
  decref r.1
  tail call show(n.2)
}}
fn inc(x.0: tobj) -> tobj {{
  let n.1: int = unbox x.0
  decref x.0
  let y.2: int = extern \"Prelude.Num Int.+\"(n.1, 1)
  let r.3: tobj = box y.2
  return r.3
}}
{SHOW_INT}"
    );
    let (out, stats) = run_lir_stats(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "11\n");
    assert_eq!(stats.conversions, 0, "{stats:?}");
}

/// 捕まえた `float` は生の語のまま持つので、箱を作らない。箱は一様な引数と結果の2つだけである
/// (docs/spec/object-model.md の「クロージャ」)。
#[test]
fn a_closure_keeps_a_float_capture_raw() {
    let text = format!(
        "\
fn main() -> unit {{
  let c.0: tobj = closure scale(2.5)
  let b.1: tobj = box 4.0
  let r.2: tobj = apply c.0(b.1)
  let x.3: float = unbox r.2
  decref r.2
  tail call show(x.3)
}}
fn scale(k.0: float, x.1: tobj) -> tobj {{
  let y.2: float = unbox x.1
  decref x.1
  let z.3: float = extern \"Prelude.Num Float.*\"(k.0, y.2)
  let r.4: tobj = box z.3
  return r.4
}}
{SHOW_FLOAT}"
    );
    let (out, stats) = run_lir_stats(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "10.0\n");
    assert_eq!((stats.boxes, stats.conversions), (2, 0), "{stats:?}");
}

/// 引数の足りない `apply` で VM が作るクロージャは、足す引数が一様なので語を写して並べる。参照の引数も即値の箱も
/// 走査する語に入る。
#[test]
fn a_partial_application_by_the_vm_copies_uniform_words() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"sum \"
  let b.1: tobj = box 1
  let c.2: tobj = apply &add3(s.0, b.1)
  let b.3: tobj = box 2
  let d.4: tobj = apply c.2(b.3)
  let b.5: tobj = box 3
  let r.6: tobj = apply d.4(b.5)
  let u.7: unit = extern Prelude.println(r.6)
  return u.7
}
fn add3(s.0: obj, a.1: tobj, b.2: tobj, c.3: tobj) -> obj {
  let x.4: int = unbox a.1
  decref a.1
  let y.5: int = unbox b.2
  decref b.2
  let z.6: int = unbox c.3
  decref c.3
  let t.7: int = extern \"Prelude.Num Int.+\"(x.4, y.5)
  let n.8: int = extern \"Prelude.Num Int.+\"(t.7, z.6)
  let m.9: obj = extern \"Prelude.Show Int.show\"(n.8)
  let r.10: obj = extern \"Prelude.++\"(s.0, m.9)
  return r.10
}
";
    let (out, stats) = run_lir_stats(text);
    let stats = stats.unwrap();
    assert_eq!(out, "sum 6\n");
    assert_eq!(stats.conversions, 0, "{stats:?}");
}

/// `make` が返すクロージャ。`int` の捕獲を生の語で持ち、後の引数を一様に受ける。
const MAKE: &str = "\
fn make(x.0: tobj) -> tobj {
  let n.1: int = unbox x.0
  decref x.0
  let c.2: tobj = closure add(n.1)
  return c.2
}
fn add(a.0: int, s.1: obj, b.2: tobj) -> unit {
  let y.3: int = unbox b.2
  decref b.2
  let n.4: int = extern \"Prelude.Num Int.+\"(a.0, y.3)
  let m.5: obj = extern \"Prelude.Show Int.show\"(n.4)
  let r.6: obj = extern \"Prelude.++\"(s.1, m.5)
  let u.7: unit = extern Prelude.println(r.6)
  return u.7
}
";

/// 1つの `apply` が `make` と、`make` の返すクロージャの両方に引数を渡す。余る引数は `ApplyRest` のフレームに
/// 預ける。`leave` の中で `exit` すると、巻き戻しはそのフレームの参照の引数を手放す。参照の引数は `++` で作った
/// 文字列なので、手放さなければ検査モードがリークとして報告する。`verifier` の拒む関数は巻き戻しの表を持たないので、
/// `main` は末尾の `apply` で自分のフレームを `ApplyRest` のフレームに替える。
#[test]
fn apply_rest_releases_its_reference_arguments_on_unwinding() {
    let text = format!(
        "\
fn main() -> unit {{
  let s.0: obj = const \"sum \"
  let a.1: tobj = box 1
  let b.2: tobj = box 2
  let u.3: unit = apply &make(a.1, s.0, b.2)
  return u.3
}}
{MAKE}"
    );
    let (out, stats) = run_lir_stats(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "sum 3\n");
    assert_eq!(stats.conversions, 0, "{stats:?}");

    let exiting = format!(
        "\
fn main() -> unit {{
  let a.0: obj = const \"sum\"
  let b.1: obj = const \" \"
  let s.2: obj = extern \"Prelude.++\"(a.0, b.1)
  let x.3: tobj = box 1
  let y.4: tobj = box 2
  tail apply &leave(x.3, s.2, y.4)
}}
fn leave(x.0: tobj) -> tobj {{
  let z.1: unit = extern Prelude.exit(3)
  let n.2: int = unbox x.0
  decref x.0
  let c.3: tobj = closure add(n.2)
  return c.3
}}
{MAKE}"
    );
    let (out, stats) = run_lir_stats(&exiting);
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
  let r.0: float = apply &half(4)
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

/// 直接の末尾呼び出しの結果の変換は、コード生成が静的に決める。`int` を返す関数を `float` を返す関数が末尾で呼ぶのは
/// 型の誤りで、その変換はない。コード生成は変換のない命令を出さずに、内部の誤りにする。
#[test]
fn a_tail_call_between_reprs_of_different_types_is_not_compiled() {
    let text = "\
fn main() -> unit {
  let x.0: float = call g(())
  return ()
}
fn g(u.0: unit) -> float {
  tail call f(u.0)
}
fn f(u.0: unit) -> int {
  return 1
}
";
    let (_, result) = run_unverified(text);
    assert_eq!(
        result.unwrap_err().to_string(),
        "internal error: a tail call converts between the reprs of different types in `g`"
    );
}

/// 変換の命令列はどの関数にも属さないので、その戻りは最後に戻った関数を替えない。`leave` は `evv` を残して `int` を
/// 返し、変換のフレームが箱に入れて `Finish` へ戻る。`Finish` で起きた誤りには、変換でなく `leave` の名前を付ける。
/// `evv` を残すのは verifier の見ない誤りなので、VM 自身の見張りを確かめるため通さずに実行する。
#[test]
fn a_conversion_return_keeps_the_function_that_returned_last() {
    let text = "\
effect E { op(unit) -> unit }
fn main() -> tobj {
  tail apply &leave(1)
}
fn leave(x.0: int) -> int {
  let w.1: tobj = prim evv_get()
  dup w.1
  let e.2: obj = prim ev_new[E]((), w.1)
  let w.3: tobj = prim evv_push(w.1, e.2)
  let u.4: unit = prim evv_set(w.3)
  return x.0
}
";
    let (_, result) = run_unverified(text);
    assert_eq!(
        result.unwrap_err().to_string(),
        "internal error: the evidence list or a yield is left at the end in `leave`"
    );
}

/// 値として使うコンストラクタは型ごとの instance (`con$Some@[Int]`) で、一様な ABI を持つので、`apply` は変換しない
/// (docs/spec/lir.md の「関数の ABI」)。`box` は `apply` に渡す `Int` の箱で、`Some` のフィールドはその箱を
/// そのまま受ける。
#[test]
fn a_constructor_value_applied_through_apply_does_not_convert() {
    let text = "data Box a = | Box a\n\nhide : Int -> a -> <Panic> Box a\nhide n x = if n <= 0 then Box x else hide (n - 1) x\n\nget : Option Int -> Int\nget o = match o with\n  | Some n -> n\n  | None -> 0\n\nloop : (Int -> Option Int) -> Int -> Int -> <Panic> Int\nloop f i acc = if i == 0 then acc else loop f (i - 1) (acc + get (f i))\n\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  let Box f = hide 0 Some\n  println (show (loop f 1000 0))";
    let (out, result) = eml_test_support::run_stats(text);
    assert_eq!(out, "500500\n", "{result:?}");
    let stats = result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        (stats.conversions, stats.boxes, stats.unboxes),
        (0, 1000, 1000),
        "{stats:?}"
    );
}
