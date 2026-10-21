//! 型どおりの `apply` と、実行時の変換 (docs/spec/vm.md の「シグネチャ」)。
//! 関数は一様でなく、`apply` の場所は引数と結果の Repr を持つ。速い道では箱を作らず、呼ぶ所と関数の Repr が合わない
//! 引数だけを `box` か `unbox` で変える。捕まえた値と部分適用の引数は関数の引数の Repr のまま持つ。

use crate::common::{disasm, run_lir_stats, run_unverified};

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

const HALF: &str = "\
fn half(x.0: float) -> float {
  let y.1: float = extern \"Prelude.Div Float./\"(x.0, 2.0)
  return y.1
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
    let (out, stats) = run_lir_stats(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "11\n");
    assert_eq!((stats.conversions, stats.boxes), (0, 0), "{stats:?}");
}

/// 一様な場所は引数と結果を `tobj` で渡す。`int` を受ける関数の前で引数を `unbox` し (箱は消費する)、`int` の結果は
/// 変換のフレームが箱に入れてから渡す。
#[test]
fn a_uniform_site_converts_into_and_out_of_a_typed_function() {
    let text = format!(
        "\
fn main() -> unit {{
  let b.0: tobj = box 10
  let r.1: tobj = uniform apply &inc(b.0)
  let n.2: int = unbox r.1
  decref r.1
  tail call show(n.2)
}}
{INC}{SHOW_INT}"
    );
    let (out, stats) = run_lir_stats(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "11\n");
    assert_eq!(
        (stats.conversions, stats.boxes, stats.unboxes),
        (1, 2, 2),
        "{stats:?}"
    );
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
    let (out, stats) = run_lir_stats(&text);
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
    let (out, stats) = run_lir_stats(text);
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
/// 参照の引数だけを手放す。生の語を `decref` すると、ビット列をポインタとして読んでしまう。参照の引数は `++` で作った
/// 文字列なので、手放さなければ検査モードがリークとして報告する。`verifier` の拒む関数は巻き戻しの
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
    let (out, stats) = run_lir_stats(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "sum 3\n");
    assert_eq!((stats.boxes, stats.conversions), (0, 0), "{stats:?}");

    let exiting = format!(
        "\
fn main() -> unit {{
  let a.0: obj = const \"sum\"
  let b.1: obj = const \" \"
  let s.2: obj = extern \"Prelude.++\"(a.0, b.1)
  tail apply &leave(1: int, s.2, 2: int)
}}
fn leave(x.0: int) -> tobj {{
  let z.1: unit = extern Prelude.exit(3)
  let c.2: tobj = closure add(x.0)
  return c.2
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
    let (out, stats) = run_lir_stats(&text);
    let stats = stats.unwrap();
    // 0 から 18 までの和と 100
    assert_eq!(out, "271\n");
    assert_eq!((stats.conversions, stats.boxes), (0, 0), "{stats:?}");
}

/// 一意な箱で受けた `float` を、場所が `tobj` とする引数から渡すと、取り出した後で箱を空ける。定数の `box` は不死の
/// 物体になるので、計算した値を箱に入れる。
#[test]
fn a_boxed_argument_frees_a_unique_box() {
    let text = format!(
        "\
fn main() -> unit {{
  let x.0: float = extern \"Prelude.Num Float.+\"(2.0, 0.5)
  let b.1: tobj = box x.0
  let r.2: float = apply &half(b.1)
  tail call show(r.2)
}}
{HALF}{SHOW_FLOAT}"
    );
    let (out, stats) = run_lir_stats(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "1.25\n");
    // `show` と `println` が文字列の確保を2つと `decref` を1つする。残りは箱の確保と、変換が箱を空ける `decref` である
    assert_eq!(
        (
            stats.conversions,
            stats.boxes,
            stats.unboxes,
            stats.allocations,
            stats.rc_decrements
        ),
        (1, 1, 1, 3, 2),
        "{stats:?}"
    );
}

/// 共有の箱は取り出した後に `decref` するだけで、呼ぶ側の参照は残る。
#[test]
fn a_boxed_argument_decrefs_a_shared_box() {
    let text = format!(
        "\
fn main() -> unit {{
  let x.0: float = extern \"Prelude.Num Float.+\"(2.0, 0.5)
  let b.1: tobj = box x.0
  dup b.1
  let r.2: float = apply &half(b.1)
  let y.3: float = unbox b.1
  decref b.1
  let z.4: float = extern \"Prelude.Num Float.+\"(r.2, y.3)
  tail call show(z.4)
}}
{HALF}{SHOW_FLOAT}"
    );
    let (out, stats) = run_lir_stats(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "3.75\n");
    assert_eq!(
        (stats.conversions, stats.boxes, stats.unboxes),
        (1, 1, 2),
        "{stats:?}"
    );
}

/// 型どおりの場所の `float` と `int` を一様な関数に渡すと、それぞれ箱に入れる。`float` の箱は物体を確保する。
#[test]
fn a_typed_site_boxes_into_a_uniform_function() {
    let text = format!(
        "\
fn main() -> unit {{
  let r.0: tobj = apply &first(2.5: float, 7: int)
  let x.1: float = unbox r.0
  decref r.0
  tail call show(x.1)
}}
fn first(a.0: tobj, b.1: tobj) -> tobj {{
  decref b.1
  return a.0
}}
{SHOW_FLOAT}"
    );
    let (out, stats) = run_lir_stats(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "2.5\n");
    assert_eq!(
        (stats.conversions, stats.boxes, stats.unboxes),
        (1, 2, 1),
        "{stats:?}"
    );
    // `show` と `println` の確保が2つ、`float` の箱が1つ。`int` の 7 は即値の箱である
    assert_eq!(stats.allocations, 3, "{stats:?}");
}

/// `int` を返す `boxed` を一様な結果の関数として呼ぶ。
const BOXED: &str = "\
fn boxed(x.0: int) -> tobj {
  let y.1: int = extern \"Prelude.Num Int.+\"(x.0, 1)
  let b.2: tobj = box y.1
  return b.2
}
";

/// 呼ぶ所の結果が `int` で、関数の結果が `tobj` なら、`apply` は変換のフレームを1つ積んでから入る。関数が戻ると、
/// 変換のフレームが箱から取り出して呼ぶ所の結果のスロットに書く。
#[test]
fn a_result_mismatch_pushes_one_conversion_frame() {
    let text = format!(
        "\
fn main() -> unit {{
  let r.0: int = apply &boxed(20: int)
  tail call show(r.0)
}}
{BOXED}{SHOW_INT}"
    );
    let (out, stats) = run_lir_stats(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "21\n");
    assert_eq!(
        (stats.conversions, stats.boxes, stats.unboxes),
        (1, 1, 1),
        "{stats:?}"
    );
}

/// 一様な場所の結果は `tobj` なので、`float` を返す関数の結果を変換のフレームが箱に入れる。
#[test]
fn a_uniform_site_boxes_the_result_of_a_typed_function() {
    let text = format!(
        "\
fn main() -> unit {{
  let x.0: float = extern \"Prelude.Num Float.+\"(2.0, 0.5)
  let b.1: tobj = box x.0
  let r.2: tobj = uniform apply &half(b.1)
  let y.3: float = unbox r.2
  decref r.2
  tail call show(y.3)
}}
{HALF}{SHOW_FLOAT}"
    );
    let (out, stats) = run_lir_stats(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "1.25\n");
    // 引数の `unbox` と結果の `box` が1つの `apply` の変換である
    assert_eq!(
        (stats.conversions, stats.boxes, stats.unboxes),
        (1, 2, 2),
        "{stats:?}"
    );
}

/// `even` は `int` を、`odd` は `tobj` を返し、互いを末尾の `apply` で呼ぶ。`even` から `odd` へは変換のフレームを積み、
/// `odd` から `even` へはそのフレームと合成して恒等になるので外す。そのため、フレームの数は反復の数によらず、`box` と
/// `unbox` は最後の結果の1回ずつだけである。
fn even_odd(n: u64) -> String {
    format!(
        "\
layout Bool {{ False, True }}
fn main() -> unit {{
  let r.0: int = apply &even({n}: int)
  tail call show(r.0)
}}
fn even(n.0: int) -> int {{
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 0)
  switch d.1 Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let m.2: int = extern \"Prelude.Num Int.-\"(n.0, 1)
  tail apply &odd(m.2)
b2:
  return 0
}}
fn odd(n.0: int) -> tobj {{
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 0)
  switch d.1 Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let m.2: int = extern \"Prelude.Num Int.-\"(n.0, 1)
  tail apply &even(m.2)
b2:
  let b.3: tobj = box 1
  return b.3
}}
{SHOW_INT}"
    )
}

#[test]
fn tail_applies_compose_conversions() {
    let run = |n: u64| {
        let (out, stats) = run_lir_stats(&even_odd(n));
        // n が奇数なら `odd(0)` が箱の 1 を返す
        assert_eq!(out, "1\n");
        stats.unwrap()
    };
    let (short, long) = (run(1001), run(2001));
    assert_eq!(short.peak_stack, long.peak_stack, "{short:?} {long:?}");
    assert_eq!(
        (long.conversions, long.boxes, long.unboxes),
        (2001, 1, 1),
        "{long:?}"
    );
}

/// `even_odd` の輪を直接の末尾呼び出しで書く。結果の変換はコード生成が命令に持たせ、実行時の変換に数えない。`even`
/// から `odd` へは変換のフレームを積み、`odd` から `even` へはそのフレームと合成して恒等になるので外す。そのため、
/// フレームの数は反復の数によらない。
#[test]
fn direct_tail_calls_compose_their_conversions() {
    let text = |n: u64| {
        format!(
            "\
layout Bool {{ False, True }}
fn main() -> unit {{
  let r.0: int = call even({n})
  tail call show(r.0)
}}
fn even(n.0: int) -> int {{
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 0)
  switch d.1 Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let m.2: int = extern \"Prelude.Num Int.-\"(n.0, 1)
  tail call odd(m.2)
b2:
  return 0
}}
fn odd(n.0: int) -> tobj {{
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 0)
  switch d.1 Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let m.2: int = extern \"Prelude.Num Int.-\"(n.0, 1)
  tail call even(m.2)
b2:
  let b.3: tobj = box 1
  return b.3
}}
{SHOW_INT}"
        )
    };
    let shown = disasm(&text(1));
    assert!(
        shown.contains("tail_call odd(s0) convert word -> int"),
        "{shown}"
    );
    assert!(
        shown.contains("tail_call even(s0) convert int -> word"),
        "{shown}"
    );
    let run = |n: u64| {
        let (out, stats) = run_lir_stats(&text(n));
        assert_eq!(out, "1\n");
        stats.unwrap()
    };
    let (short, long) = (run(1001), run(2001));
    assert_eq!(short.peak_stack, long.peak_stack, "{short:?} {long:?}");
    assert_eq!(
        (long.conversions, long.boxes, long.unboxes),
        (0, 1, 1),
        "{long:?}"
    );
}

/// 操作の結果が `int` の末尾の `clause` は、`tobj` を返す `walk` から変換のフレームを積む。節の関数 `stepper` は
/// `int` を返し、`tobj` を返す `walk` を末尾で呼ぶので、その変換はフレームと合成して恒等になり、フレームを外す。
/// そのため、`walk` と節の関数の輪はスタックを伸ばさない。
#[test]
fn a_tail_clause_composes_its_conversion() {
    let text = |n: u64| {
        format!(
            "\
layout Bool {{ False, True }}
effect Step {{ step(int) -> int }}
fn main() -> unit {{
  let w.0: tobj = prim evv_get()
  dup w.0
  dup w.0
  let e.1: obj = prim ev_new[Step]((), w.0)
  let u.2: unit = prim ev_set_clause[Step.step](e.1, &stepper)
  dup e.1
  let w.3: tobj = prim evv_push(w.0, e.1)
  let u.4: unit = prim evv_set(w.3)
  let r.5: tobj = call walk(e.1, {n})
  let u.6: unit = prim evv_set(w.0)
  let k.7: int = unbox r.5
  decref r.5
  tail call show(k.7)
}}
yields fn walk(e.0: obj, n.1: int) -> tobj {{
  let d.2: enum = extern \"Prelude.Eq Int.==\"(n.1, 0)
  switch d.2 Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let c.3: tobj = prim ev_clause[Step.step](e.0)
  tail clause Step.step c.3(e.0, n.1)
b2:
  decref e.0
  let b.4: tobj = box 7
  return b.4
}}
internal yields clause Step.step fn stepper(e.0: obj, n.1: int) -> int {{
  let m.2: int = extern \"Prelude.Num Int.-\"(n.1, 1)
  tail call walk(e.0, m.2)
}}
{SHOW_INT}"
        )
    };
    let shown = disasm(&text(1));
    assert!(
        shown.contains("tail_clause s2(s0, s1) convert int -> word"),
        "{shown}"
    );
    assert!(
        shown.contains("tail_call walk(s0, s1) convert word -> int"),
        "{shown}"
    );
    let run = |n: u64| {
        let (out, stats) = run_lir_stats(&text(n));
        assert_eq!(out, "7\n");
        stats.unwrap()
    };
    let (short, long) = (run(1000), run(2000));
    assert_eq!(short.peak_stack, long.peak_stack, "{short:?} {long:?}");
    assert_eq!(long.conversions, 0, "{long:?}");
}

/// `f` は `tobj` を返し、`int` の場所から入る。`f` が `int` を返す `g` を末尾で `apply` すると、変換は `int` から
/// `int` への恒等になるので、変換のフレームを外し、`g` が `main` の見出しを引き継ぐ。`g` の中の `exit` が外すのは
/// `g` のフレームだけである。
#[test]
fn a_composed_identity_pops_the_frame() {
    let text = |exit: &str| {
        format!(
            "\
fn main() -> int {{
  tail apply &f(5: int)
}}
fn f(x.0: int) -> tobj {{
  tail apply &g(x.0)
}}
fn g(x.0: int) -> int {{
  let s.1: obj = extern \"Prelude.Show Int.show\"(x.0)
  let u.2: unit = extern Prelude.println(s.1)
{exit}  return x.0
}}
"
        )
    };
    let (out, stats) = run_lir_stats(&text(""));
    let stats = stats.unwrap();
    assert_eq!(out, "5\n");
    assert_eq!(
        (stats.conversions, stats.boxes, stats.unboxes),
        (2, 0, 0),
        "{stats:?}"
    );
    // `apply` の変換は実行時に決まるので、末尾の `apply` の命令は変換を持たない
    let shown = disasm(&text(""));
    assert!(!shown.contains("convert"), "{shown}");

    let (out, stats) = run_lir_stats(&text("  let z.3: unit = extern Prelude.exit(0)\n"));
    let stats = stats.unwrap();
    assert_eq!(out, "5\n");
    assert_eq!(stats.unwound_frames, 1, "{stats:?}");
}

/// 結果の Repr が呼び出し元の `ret` と違う直接の末尾呼び出しは、コード生成が決めた変換のフレームを積む。設計どおりの
/// 変換なので `conversions` には数えない。
#[test]
fn a_direct_tail_call_converts_its_result() {
    let text = format!(
        "\
fn main() -> unit {{
  let r.0: tobj = call wrap(41)
  let n.1: int = unbox r.0
  decref r.0
  tail call show(n.1)
}}
fn wrap(x.0: int) -> tobj {{
  tail call inc(x.0)
}}
{INC}{SHOW_INT}"
    );
    assert!(
        disasm(&text).contains("tail_call inc(s0) convert int -> word"),
        "{}",
        disasm(&text)
    );
    let (out, stats) = run_lir_stats(&text);
    let stats = stats.unwrap();
    assert_eq!(out, "42\n");
    assert_eq!(
        (stats.conversions, stats.boxes, stats.unboxes),
        (0, 1, 1),
        "{stats:?}"
    );
}

/// yield 中に戻った値は使われずに捨てられるので、変換のフレームは値を箱に入れず、`tobj` のダミーを書く。`leave` は
/// yield を始めて `float` のダミーを返す。
#[test]
fn yielding_through_a_conversion_frame_writes_a_dummy() {
    let text = |yield_to: &str| {
        format!(
            "\
effect E {{ op(unit) -> unit }}
fn main() -> unit {{
  let w.0: tobj = prim evv_get()
  let e.1: obj = prim ev_new[E]((), w.0)
  let r.2: tobj = uniform apply &leave(e.1)
  decref r.2
{yield_to}  let s.5: obj = const \"done\"
  let u.6: unit = extern Prelude.println(s.5)
  return u.6
}}
fn leave(e.0: obj) -> float {{
{}  return 0.0
}}
",
            if yield_to.is_empty() {
                "  decref e.0\n"
            } else {
                "  let u.1: unit = prim yield_to(e.0, (), ())\n"
            }
        )
    };
    let finish = "  let k.3: tobj = prim yield_cont_take()
  decref k.3
  let c.4: tobj = prim yield_finish()
  decref c.4
";
    let (out, stats) = run_lir_stats(&text(finish));
    let stats = stats.unwrap();
    assert_eq!(out, "done\n");
    assert_eq!((stats.conversions, stats.boxes), (1, 0), "{stats:?}");

    // yield しなければ、同じ変換のフレームが `0.0` を箱に入れる
    let (out, stats) = run_lir_stats(&text(""));
    let stats = stats.unwrap();
    assert_eq!(out, "done\n");
    assert_eq!((stats.conversions, stats.boxes), (1, 1), "{stats:?}");
}

/// `never` の節の入口に当たる `abort` が、変換のフレームの下で中断する。`middle` の末尾の `apply` は `int` を返す
/// `abort` に変換のフレームを積むので、巻き戻しは `abort` と変換のフレームを外して `main` の `apply` に着地する。
/// 変換のフレームは何も持たない。`abort` のフレームは文字列を持つので、手放さなければ検査モードがリークとして報告する。
#[test]
fn an_abort_unwinds_a_conversion_frame() {
    let text = "\
effect E { never op(unit) -> never }
fn main() -> unit {
  let w.0: tobj = prim evv_get()
  let e.1: obj = prim ev_new[E]((), w.0)
  dup e.1
  let u.2: tobj = uniform apply &middle(e.1) prompt e.1
  decref u.2
  decref e.1
  let c.3: tobj = prim yield_finish()
  decref c.3
  let s.4: obj = const \"landed\"
  let u.5: unit = extern Prelude.println(s.4)
  return u.5
}
fn middle(e.0: obj) -> tobj {
  tail apply &abort(e.0)
}
fn abort(e.0: obj) -> int {
  let a.1: obj = const \"held\"
  let b.2: obj = const \" here\"
  let s.3: obj = extern \"Prelude.++\"(a.1, b.2)
  let u.4: unit = prim yield_abort(e.0, ())
  let v.5: unit = extern Prelude.println(s.3)
  return 0
}
";
    let (out, stats) = run_lir_stats(text);
    let stats = stats.unwrap();
    assert_eq!(out, "landed\n");
    // `abort` と変換のフレーム
    assert_eq!(stats.unwound_frames, 2, "{stats:?}");
}

/// 変換の命令列はどの関数にも属さないので、その戻りは最後に戻った関数を替えない。`leave` は `evv` を残して `int` を
/// 返し、変換のフレームが箱に入れて `Finish` へ戻る。`Finish` で起きた誤りには、変換でなく `leave` の名前を付ける。
/// `evv` を残すのは verifier の見ない誤りなので、VM 自身の見張りを確かめるため通さずに実行する。
#[test]
fn a_conversion_return_keeps_the_function_that_returned_last() {
    let text = "\
effect E { op(unit) -> unit }
fn main() -> tobj {
  tail apply &leave(1: int)
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

/// 変換の道を確かめる UI テストのプログラムが、実際にその道を通ることを確かめる。UI テストは出力だけを比べるので、
/// optimize が関数の値を特殊化すると、変換を通らなくなっても気づけないためである。`panic_through_conversion` は
/// 誤りで止まり `RunStats` を返さないので、ゼロ除算を除いた同じ道の実行で数える。
#[test]
fn ui_programs_take_the_conversion_paths_they_describe() {
    let ui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/ui");
    let stats = |path: &str, edit: Option<(&str, &str)>, expected: &str| {
        let mut text = std::fs::read_to_string(ui.join(path))
            .unwrap_or_else(|error| panic!("{path}: {error}"));
        if let Some((from, to)) = edit {
            text = text.replace(from, to);
        }
        let (out, result) = eml_test_support::run_stats(&text);
        assert_eq!(out, expected, "{path}: {result:?}");
        result.unwrap_or_else(|error| panic!("{path}: {error}"))
    };
    let none = None;
    for (path, edit, expected) in [
        (
            "run/functions/polymorphic_recursion_with_function_values.em",
            none,
            "20\n4.5\nhi!!\n",
        ),
        (
            "run/functions/function_value_made_inside_a_uniform_instance.em",
            none,
            "5\n1.5\nfirst\n",
        ),
        (
            "run/functions/function_values_inside_data_called_generically.em",
            none,
            "15\n2.0\nb-a\n",
        ),
        (
            "run/unwind/abort_through_conversion.em",
            none,
            "Ok 10\nErr \"zero\"\n",
        ),
        (
            "run-fail/panic/panic_through_conversion.em",
            Some(("divide 0", "divide 1")),
            "20\n100\n",
        ),
    ] {
        let stats = stats(path, edit, expected);
        assert!(stats.conversions > 0, "{path}: {stats:?}");
    }
    // 捕まえた `Float` と部分適用の `Float` は生の語のまま持つので、変換も箱もない
    let stats = stats(
        "run/functions/float_closure_partial_application.em",
        none,
        "3.75\n2.25\n7.5\n",
    );
    assert_eq!((stats.conversions, stats.boxes), (0, 0), "{stats:?}");
}

/// 値として使うコンストラクタは型ごとの instance (`con$Some@[Int]`) なので、型どおりの `apply` は速い道を通る。
/// スキームから決めた包む関数 (引数が `tobj`) では、呼ぶたびに引数を変換していた
/// (docs/spec/lir.md の「関数の ABI」)。残る `box` は `Some` の
/// フィールドに `Int` を入れる箱である。
#[test]
fn a_constructor_value_applied_at_its_type_does_not_convert() {
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
