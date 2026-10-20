//! box の挿入のパス (docs/spec/lir.md の「box の挿入」)。手で書いた LIR を入力にする。ソースからパイプラインを通す
//! テストは `eml_core_ir` の tests/boxing.rs にある。

use crate::common::function;
use eml_lir::{Pass, boxing, parse, passes_until, pretty, verify_lowered, verify_scopes};

/// 入力が入口の段の verifier を、出力が範囲の段の verifier を通ることも確かめる。
fn boxing_text(text: &str) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = verify_lowered(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    boxing(&mut program);
    let shown = pretty(&program);
    if let Err(error) = verify_scopes(&program) {
        panic!("the output must verify: {error}\n{shown}");
    }
    shown
}

#[test]
fn a_value_boxed_by_the_pass_is_passed_on_without_a_new_unbox() {
    // `int` の結果を受け直して `box` した変数を `int` の位置へ渡すときは、`unbox` を作らずに受け直した変数を渡す。
    // translate の出力には現れない形なので、IR のテキストで確かめる
    let text = "\
fn g() -> int {
  return 1
}
fn h(n.0: int) -> int {
  return n.0
}
fn f() -> int {
  let r.0: tobj = call g()
  let s.1: int = call h(r.0)
  return s.1
}
";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    fn f() -> int {
      let r.2: int = call g()
      let r.0: tobj = box r.2
      let s.1: int = call h(r.2)
      return s.1
    }
    ");
}

#[test]
fn a_typed_apply_converts_only_a_constant_to_its_entry() {
    // 型どおりの場所は引数と結果を場所の Repr で渡すので、変数の引数も結果も変換しない。場所が `tobj` とする定数だけを
    // `box` し、場所の Repr は変換した後の値を表す
    // (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「`apply` の ABI」)
    let text = "\
fn f(c.0: tobj, n.1: int) -> int {
  let r.2: int = apply c.0(n.1, 5: tobj)
  let s.3: int = extern \"Prelude.Num Int.+\"(r.2, 1)
  return s.3
}
";
    insta::assert_snapshot!(boxing_text(text), @r#"
    fn f(c.0: tobj, n.1: int) -> int {
      let b.4: tobj = box 5
      let r.2: int = apply c.0(n.1, b.4)
      let s.3: int = extern "Prelude.Num Int.+"(r.2, 1)
      return s.3
    }
    "#);
}

#[test]
fn a_uniform_site_converts_its_arguments_and_result() {
    // EP の一様な場所は引数も結果も `tobj` である。結果を `tobj` の新しい変数で受け、元の変数を直後の `unbox` で
    // 定義する。使う所の変数は書き換えない
    let text = "\
fn f(c.0: tobj, n.1: int) -> int {
  let r.2: int = uniform apply c.0(n.1)
  let s.3: int = extern \"Prelude.Num Int.+\"(r.2, 1)
  return s.3
}
";
    insta::assert_snapshot!(boxing_text(text), @r#"
    fn f(c.0: tobj, n.1: int) -> int {
      let n.4: tobj = box n.1
      let r.5: tobj = uniform apply c.0(n.4)
      let r.2: int = unbox r.5
      let s.3: int = extern "Prelude.Num Int.+"(r.2, 1)
      return s.3
    }
    "#);
}

#[test]
fn a_direct_call_to_a_lambda_converts_nothing() {
    // 内部の関数も型どおりの ABI を持つので、展開しないラムダを直接呼ぶ所に `box` も `unbox` も入らない
    // (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「関数の ABI」)
    let text = "\
internal fn main$lambda0(x.0: float) -> float {
  let y.1: float = extern \"Prelude.Num Float.+\"(x.0, 1.0)
  return y.1
}
fn main(x.0: float) -> float {
  let r.1: float = call main$lambda0(x.0)
  let s.2: float = extern \"Prelude.Num Float.+\"(r.1, 2.0)
  return s.2
}
";
    insta::assert_snapshot!(boxing_text(text), @r#"
    internal fn main$lambda0(x.0: float) -> float {
      let y.1: float = extern "Prelude.Num Float.+"(x.0, 1.0)
      return y.1
    }
    fn main(x.0: float) -> float {
      let r.1: float = call main$lambda0(x.0)
      let s.2: float = extern "Prelude.Num Float.+"(r.1, 2.0)
      return s.2
    }
    "#);
}

#[test]
fn a_role_boxes_only_its_uniform_positions() {
    // 節の関数は `k` と状態と結果だけが一様である。捕まえた `n` と操作の引数 `x` は `int` のまま受ける
    // (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「関数の ABI」)
    let text = "\
internal role clause fn g(n.0: int, x.1: int, k.2: tobj, s.3: int) -> int {
  let t.4: int = extern \"Prelude.Num Int.+\"(n.0, x.1)
  let r.5: int = extern \"Prelude.Num Int.+\"(t.4, s.3)
  return r.5
}
";
    insta::assert_snapshot!(boxing_text(text), @r#"
    internal role clause fn g(n.0: int, x.1: int, k.2: tobj, s.6: tobj) -> tobj {
      let s.3: int = unbox s.6
      let t.4: int = extern "Prelude.Num Int.+"(n.0, x.1)
      let r.5: int = extern "Prelude.Num Int.+"(t.4, s.3)
      let r.7: tobj = box r.5
      return r.7
    }
    "#);
}

#[test]
fn closure_arguments_convert_to_the_callee_params() {
    // `closure` の引数は直接の呼び出しの引数と同じく、呼ばれる関数の引数の Repr へ変換する。`_` の instance の部分
    // 適用では、`int` の `5` を `box` して捕まえる
    let text = "\
fn \"depth@[_]\"(x.0: tobj, n.1: int) -> int {
  return n.1
}
fn main() -> int {
  let c.0: tobj = closure \"depth@[_]\"(5)
  let r.1: int = apply c.0(2: int)
  return r.1
}
";
    insta::assert_snapshot!(function(&boxing_text(text), "main"), @r#"
    fn main() -> int {
      let b.2: tobj = box 5
      let c.0: tobj = closure "depth@[_]"(b.2)
      let r.1: int = apply c.0(2: int)
      return r.1
    }
    "#);
}

#[test]
fn an_incompatible_tail_call_becomes_tail() {
    // `g` は `tobj` を返し、`f` は `int` を返す。末尾の位置の呼び出しの束縛と `return` には変換を入れないので、縮約が
    // `tail` にする。結果の変換は VM の変換のフレームが行う
    // (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「box の挿入」)
    let text = "\
fn g(x.0: tobj) -> tobj {
  return x.0
}
fn f(n.0: int) -> int {
  let r.1: int = call g(n.0)
  return r.1
}
";
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    passes_until(&mut program, Pass::Contract);
    insta::assert_snapshot!(function(&pretty(&program), "f"), @"
    fn f(n.0: int) -> int {
      let n.2: tobj = box n.0
      tail call g(n.2)
    }
    ");
}

#[test]
#[should_panic(expected = "internal error: the boxing pass cannot pass 5 to obj in `f`")]
fn a_constant_that_cannot_go_to_its_position_is_an_internal_error() {
    // 変換の段は直接の呼び出しの引数を比べないので、`obj` の引数に渡す `Int` の定数は box の挿入まで届く。型からは
    // 起きない変換なので、パスはパニックする (docs/spec/lir.md の「box の挿入」)
    let text = "\
fn h(x.0: obj) -> int {
  return 1
}
fn f() -> int {
  let r.0: int = call h(5)
  return r.0
}
";
    boxing_text(text);
}

#[test]
fn prim_operands_are_converted_like_call_arguments() {
    // `ev_new` は状態を `tobj` で受けるので、`int` の状態は `box` して渡す
    let text = "effect State { get(unit) -> tobj, put(tobj) -> unit }\nfn f(n.0: int) -> obj {\n  let w.1: tobj = prim evv_get()\n  let e.2: obj = prim ev_new[State](n.0, w.1)\n  return e.2\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    fn f(n.0: int) -> obj {
      let w.1: tobj = prim evv_get()
      let n.3: tobj = box n.0
      let e.2: obj = prim ev_new[State](n.3, w.1)
      return e.2
    }
    ");
}

#[test]
fn a_prim_result_is_rebound_to_its_row_repr() {
    // `yielding` の結果は `int` なので、`tobj` の変数で受けるなら `int` の新しい変数で受けて `box` する
    let text = "fn f() -> tobj {\n  let y.0: tobj = prim yielding()\n  return y.0\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    fn f() -> tobj {
      let y.1: int = prim yielding()
      let y.0: tobj = box y.1
      return y.0
    }
    ");
}

#[test]
fn a_clause_call_uses_the_reprs_of_its_operation() {
    // `put` の `int` の引数へ `tobj` の n.2 を `unbox` して渡す。`get` は末尾の位置で呼ぶので、結果を受け直さず、
    // 束縛を操作の `int` にする
    let text = "effect Counter { get(unit) -> int, put(int) -> unit }\nyields fn f(c.0: tobj, e.1: obj, n.2: tobj) -> tobj {\n  let u.3: unit = clause Counter.put c.0(e.1, n.2)\n  let c.4: tobj = prim ev_clause[Counter.get](e.1)\n  let m.5: tobj = clause Counter.get c.4(e.1, ())\n  return m.5\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    yields fn f(c.0: tobj, e.1: obj, n.2: tobj) -> tobj {
      let n.6: int = unbox n.2
      let u.3: unit = clause Counter.put c.0(e.1, n.6)
      let c.4: tobj = prim ev_clause[Counter.get](e.1)
      let m.5: int = clause Counter.get c.4(e.1, ())
      return m.5
    }
    ");
}

#[test]
fn a_clause_function_takes_the_reprs_of_its_operation() {
    // 引数 n は `put` の `int` になり、入口で元の `tobj` の変数を `box` で定義する。`ret` は `unit` になる
    let text = "effect Counter { get(unit) -> int, put(int) -> unit }\ninternal yields clause Counter.put fn g(e.0: obj, n.1: tobj) -> tobj {\n  return ()\n}\nfn main() -> unit {\n  let w.0: tobj = prim evv_get()\n  let e.1: obj = prim ev_new[Counter](0, w.0)\n  let u.2: unit = prim ev_set_clause[Counter.put](e.1, &g)\n  return ()\n}\n";
    let shown = boxing_text(text);
    insta::assert_snapshot!(function(&shown, "g"), @"
    internal yields clause Counter.put fn g(e.0: obj, n.2: int) -> unit {
      let n.1: tobj = box n.2
      return ()
    }
    ");
    insta::assert_snapshot!(function(&shown, "main"), @"
    fn main() -> unit {
      let w.0: tobj = prim evv_get()
      let b.3: tobj = box 0
      let e.1: obj = prim ev_new[Counter](b.3, w.0)
      let u.2: unit = prim ev_set_clause[Counter.put](e.1, &g)
      return ()
    }
    ");
}

#[test]
fn a_clause_function_keeps_its_captures_uniform() {
    // `int` の捕獲は `tobj` で受けて入口で `unbox` し、`closure` は捕獲を `box` して渡す。evidence と `get` の引数と
    // `ret` はすでに操作の表と合っている
    let text = "effect Counter { get(unit) -> int, put(int) -> unit }\ninternal yields clause Counter.get fn g(k.0: int, e.1: obj, u.2: unit) -> int {\n  return k.0\n}\nfn main(k.0: int) -> unit {\n  let w.1: tobj = prim evv_get()\n  let e.2: obj = prim ev_new[Counter]((), w.1)\n  let g.3: tobj = closure g(k.0)\n  let u.4: unit = prim ev_set_clause[Counter.get](e.2, g.3)\n  return ()\n}\n";
    let shown = boxing_text(text);
    insta::assert_snapshot!(function(&shown, "g"), @"
    internal yields clause Counter.get fn g(k.3: tobj, e.1: obj, u.2: unit) -> int {
      let k.0: int = unbox k.3
      return k.0
    }
    ");
    insta::assert_snapshot!(function(&shown, "main"), @"
    fn main(k.0: int) -> unit {
      let w.1: tobj = prim evv_get()
      let e.2: obj = prim ev_new[Counter]((), w.1)
      let k.5: tobj = box k.0
      let g.3: tobj = closure g(k.5)
      let u.4: unit = prim ev_set_clause[Counter.get](e.2, g.3)
      return ()
    }
    ");
}

#[test]
fn a_never_clause_call_is_not_rebound() {
    // `never` の操作の節の呼び出しは戻らないので、`int` の束縛を受け直さず、`return` にも変換を入れない
    let text = "effect Fail { never fail(tobj) -> never }\nyields fn f(c.0: tobj, e.1: obj, s.2: obj) -> int {\n  let r.3: int = clause Fail.fail c.0(e.1, s.2)\n  return r.3\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    yields fn f(c.0: tobj, e.1: obj, s.2: obj) -> int {
      let r.3: int = clause Fail.fail c.0(e.1, s.2)
      return r.3
    }
    ");
}

#[test]
fn the_uses_of_a_never_clause_binder_are_not_converted() {
    // `int` の束縛を `tobj` の位置 (一様な `apply` の引数と `tobj` の `ret` の `return`) へ渡しても `box` しない。使いには制御が
    // 届かないからである
    let text = "effect Fail { never fail(tobj) -> never }\nyields fn f(c.0: tobj, e.1: obj, s.2: obj, k.3: tobj) -> tobj {\n  let r.4: int = clause Fail.fail c.0(e.1, s.2)\n  let t.5: tobj = uniform apply k.3(r.4)\n  return t.5\n}\nyields fn g(c.0: tobj, e.1: obj, s.2: obj) -> tobj {\n  let r.3: int = clause Fail.fail c.0(e.1, s.2)\n  return r.3\n}\n";
    let shown = boxing_text(text);
    insta::assert_snapshot!(function(&shown, "f"), @"
    yields fn f(c.0: tobj, e.1: obj, s.2: obj, k.3: tobj) -> tobj {
      let r.4: int = clause Fail.fail c.0(e.1, s.2)
      let t.5: tobj = uniform apply k.3(r.4)
      return t.5
    }
    ");
    insta::assert_snapshot!(function(&shown, "g"), @"
    yields fn g(c.0: tobj, e.1: obj, s.2: obj) -> tobj {
      let r.3: int = clause Fail.fail c.0(e.1, s.2)
      return r.3
    }
    ");
}

#[test]
fn the_result_of_exit_is_not_converted() {
    // 戻らない extern の `int` の束縛を `tobj` を返す `return` に渡しても、受け直さず `box` もしない
    let text =
        "fn f(n.0: int) -> tobj {\n  let t.1: int = extern Prelude.exit(n.0)\n  return t.1\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    fn f(n.0: int) -> tobj {
      let t.1: int = extern Prelude.exit(n.0)
      return t.1
    }
    ");
}

#[test]
fn a_clause_function_unboxes_its_return_to_a_scalar_operation_result() {
    // `Counter.get` の結果は `int` なので、節の関数の `ret` を `int` にし、`tobj` の値を返す `return` の前で `unbox` する
    let text = "effect Counter { get(unit) -> int, put(int) -> unit }\ninternal yields clause Counter.get fn g(e.0: obj, u.1: unit) -> tobj {\n  let s.2: tobj = prim ev_take_state(e.0)\n  return s.2\n}\nfn main() -> unit {\n  let w.0: tobj = prim evv_get()\n  let e.1: obj = prim ev_new[Counter]((), w.0)\n  let u.2: unit = prim ev_set_clause[Counter.get](e.1, &g)\n  return ()\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "g"), @"
    internal yields clause Counter.get fn g(e.0: obj, u.1: unit) -> int {
      let s.2: tobj = prim ev_take_state(e.0)
      let s.3: int = unbox s.2
      return s.3
    }
    ");
}

#[test]
fn a_never_clause_function_returns_obj() {
    // `never` の操作の節の入口はダミーの物体を返すので、`ret` は `obj` になる
    let text = "effect Fail { never fail(tobj) -> never }\ninternal yields clause Fail.fail fn g(e.0: obj, s.1: tobj) -> tobj {\n  let d.2: obj = prim dummy()\n  return d.2\n}\nfn main() -> unit {\n  let w.0: tobj = prim evv_get()\n  let e.1: obj = prim ev_new[Fail]((), w.0)\n  let u.2: unit = prim ev_set_clause[Fail.fail](e.1, &g)\n  return ()\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "g"), @"
    internal yields clause Fail.fail fn g(e.0: obj, s.1: tobj) -> obj {
      let d.2: obj = prim dummy()
      return d.2
    }
    ");
}

/// case のフィールド `y` を、後の2つの枝で使う関数。`b3` は `tobj` の位置 (`con`) で、`b4` は `int` の位置
/// (`extern`) で使う。`both` が真なら、`b3` も `int` の位置で使う。
fn case_field_in_branches(both: bool) -> String {
    let b3 = if both {
        "  let s.3: int = extern \"Prelude.Num Int.+\"(y.2, 2)\n  let u.6: tobj = con Option #1(s.3)\n  return u.6\n"
    } else {
        "  let u.6: tobj = con Option #1(y.2)\n  return u.6\n"
    };
    format!(
        "\
layout Prelude.Bool {{ False, True }}
layout Option {{ None, Some(tobj) }}
fn f(o.0: tobj, c.1: enum) -> tobj {{
  switch o.0 Option {{ #0 -> b1, #1(y.2: int) -> b2 }}
b1:
  return #0
b2:
  switch c.1 Prelude.Bool {{ #0 -> b3, #1 -> b4 }}
b3:
{b3}b4:
  let t.4: int = extern \"Prelude.Num Int.+\"(y.2, 1)
  let v.5: tobj = con Option #1(t.4)
  return v.5
}}
"
    )
}

#[test]
fn a_case_field_is_unboxed_in_the_one_branch_that_uses_it_as_a_scalar() {
    // `y` を `int` で使うのは `b4` だけなので、`unbox` は case の行き先の `b2` でなく `b4` の先頭に置く。`b3` は
    // `tobj` のままのフィールドを渡す。`b3` を通る実行は `unbox` を踏まない (docs/spec/lir.md の「box の挿入」)
    insta::assert_snapshot!(function(&boxing_text(&case_field_in_branches(false)), "f"), @"
    fn f(o.0: tobj, c.1: enum) -> tobj {
      switch o.0 Option { #0 -> b1, #1(y.7: tobj) -> b2 }
    b1:
      return #0
    b2:
      switch c.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      let u.6: tobj = con Option #1(y.7)
      return u.6
    b4:
      let y.2: int = unbox y.7
      let t.4: int = extern \"Prelude.Num Int.+\"(y.2, 1)
      let t.8: tobj = box t.4
      let v.5: tobj = con Option #1(t.8)
      return v.5
    }
    ");
}

#[test]
fn a_case_field_used_as_a_scalar_in_two_branches_is_unboxed_at_their_common_dominator() {
    // `b3` と `b4` の共通の支配者で最も近いのは `b2` なので、`unbox` は `b2` の先頭に1つだけ置く
    insta::assert_snapshot!(function(&boxing_text(&case_field_in_branches(true)), "f"), @"
    fn f(o.0: tobj, c.1: enum) -> tobj {
      switch o.0 Option { #0 -> b1, #1(y.7: tobj) -> b2 }
    b1:
      return #0
    b2:
      let y.2: int = unbox y.7
      switch c.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      let s.3: int = extern \"Prelude.Num Int.+\"(y.2, 2)
      let s.8: tobj = box s.3
      let u.6: tobj = con Option #1(s.8)
      return u.6
    b4:
      let t.4: int = extern \"Prelude.Num Int.+\"(y.2, 1)
      let t.9: tobj = box t.4
      let v.5: tobj = con Option #1(t.9)
      return v.5
    }
    ");
}

#[test]
fn a_case_field_never_used_as_a_scalar_is_not_unboxed() {
    let text = "\
layout Option { None, Some(tobj) }
fn f(o.0: tobj) -> tobj {
  switch o.0 Option { #0 -> b1, #1(y.1: int) -> b2 }
b1:
  return #0
b2:
  let u.2: tobj = con Option #1(y.1)
  return u.2
}
";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    fn f(o.0: tobj) -> tobj {
      switch o.0 Option { #0 -> b1, #1(y.3: tobj) -> b2 }
    b1:
      return #0
    b2:
      let u.2: tobj = con Option #1(y.3)
      return u.2
    }
    ");
}

/// case のフィールド `y` を、`b3` の下の枝で使う関数。`b5` と `b6` は合流の `b7` へ jump する。`arg5` は `b5` が
/// `b7` に渡す値である。`b7` は `int` の引数 `t` を受け、`join` を足して返す。
fn case_field_before_a_join(arg5: &str, join: &str) -> String {
    format!(
        "\
layout Prelude.Bool {{ False, True }}
layout Option {{ None, Some(tobj) }}
fn f(o.0: tobj, c.1: enum, d.2: enum) -> int {{
  switch o.0 Option {{ #0 -> b1, #1(y.3: int) -> b2 }}
b1:
  return 0
b2:
  switch c.1 Prelude.Bool {{ #0 -> b3, #1 -> b4 }}
b3:
  switch d.2 Prelude.Bool {{ #0 -> b5, #1 -> b6 }}
b4:
  return 1
b5:
  jump b7({arg5})
b6:
  jump b7(2)
b7(t.4: int):
  let r.5: int = extern \"Prelude.Num Int.+\"(t.4, {join})
  return r.5
}}
"
    )
}

#[test]
fn a_case_field_used_after_a_join_is_unboxed_before_the_join() {
    // `y` を使う所は合流の `b7` だけである。`b7` に置くと、借りた `tobj` のフィールドが jump の行き先で生きるので、
    // Perceus が `b5` と `b6` で所有を取る (`dup`)。そのため、行き先から switch の辺だけで届く最も深い所、`b3` に置く
    // (docs/spec/lir.md の「box の挿入」)
    insta::assert_snapshot!(function(&boxing_text(&case_field_before_a_join("1", "y.3")), "f"), @"
    fn f(o.0: tobj, c.1: enum, d.2: enum) -> int {
      switch o.0 Option { #0 -> b1, #1(y.6: tobj) -> b2 }
    b1:
      return 0
    b2:
      switch c.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      let y.3: int = unbox y.6
      switch d.2 Prelude.Bool { #0 -> b5, #1 -> b6 }
    b4:
      return 1
    b5:
      jump b7(1)
    b6:
      jump b7(2)
    b7(t.4: int):
      let r.5: int = extern \"Prelude.Num Int.+\"(t.4, y.3)
      return r.5
    }
    ");
}

#[test]
fn a_case_field_passed_as_a_jump_argument_is_unboxed_in_the_jumping_block() {
    // jump の実引数は、jump するブロックでの使いである。`b5` は合流の前なので、そこに置く
    let text = case_field_before_a_join("y.3", "1");
    insta::assert_snapshot!(function(&boxing_text(&text), "f"), @"
    fn f(o.0: tobj, c.1: enum, d.2: enum) -> int {
      switch o.0 Option { #0 -> b1, #1(y.6: tobj) -> b2 }
    b1:
      return 0
    b2:
      switch c.1 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b3:
      switch d.2 Prelude.Bool { #0 -> b5, #1 -> b6 }
    b4:
      return 1
    b5:
      let y.3: int = unbox y.6
      jump b7(y.3)
    b6:
      jump b7(2)
    b7(t.4: int):
      let r.5: int = extern \"Prelude.Num Int.+\"(t.4, 1)
      return r.5
    }
    ");
}

#[test]
fn boxing_converts_extern_tobj_boundaries() {
    // 型変数の位置の引数と結果は `tobj` である。`int` の結果は受け直して `unbox` し、`int` の引数は `box` する
    // (docs/spec/lir.md の「box の挿入」)
    let text = "\
fn read(a.0: obj, i.1: int) -> int {
  let x.2: int = extern \"Std.Array.get\"(a.0, i.1)
  return x.2
}
fn fill(n.0: int, v.1: int) -> obj {
  let a.2: obj = extern \"Std.Array.make\"(n.0, v.1)
  return a.2
}
";
    let shown = boxing_text(text);
    insta::assert_snapshot!(function(&shown, "read"), @"
    fn read(a.0: obj, i.1: int) -> int {
      let x.3: tobj = extern Std.Array.get(a.0, i.1)
      let x.2: int = unbox x.3
      return x.2
    }
    ");
    insta::assert_snapshot!(function(&shown, "fill"), @"
    fn fill(n.0: int, v.1: int) -> obj {
      let v.3: tobj = box v.1
      let a.2: obj = extern Std.Array.make(n.0, v.3)
      return a.2
    }
    ");
}
