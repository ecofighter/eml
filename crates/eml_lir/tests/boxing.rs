//! box の挿入のパス (docs/spec/lir.md の「box の挿入」)。手で書いた LIR を入力にする。T3 の時間は IR のテキストで
//! 確かめる。ソースからパイプラインを通すテストは `eml_core_ir` の tests/boxing.rs にある。

use crate::common::function;
use eml_lir::{Repr, boxing, parse, pretty, verify_lowered, verify_scopes};

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
fn t3_takes_time_linear_in_the_chain() {
    // `f_i` は `f_{i+1}` を末尾の位置で呼び、最後の関数は `apply` を末尾の位置で呼ぶ。呼ばれる側の番号が大きいので、
    // 変わらなくなるまで全体を繰り返す形では、1回に1つしか上がらず2乗の時間になる
    const N: usize = 30_000;
    let mut text = String::new();
    for i in 0..N - 1 {
        text.push_str(&format!(
            "fn f{i}(x.0: int) -> int {{\n  let r.1: int = call f{}(x.0)\n  return r.1\n}}\n",
            i + 1
        ));
    }
    text.push_str(&format!(
        "fn f{}(x.0: int) -> int {{\n  let r.1: int = apply &k(x.0)\n  return r.1\n}}\n",
        N - 1
    ));
    text.push_str("fn k(x.0: tobj) -> tobj {\n  return x.0\n}\n");
    let mut program = parse(&text).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(verify_lowered(&program), Ok(()));
    let start = std::time::Instant::now();
    boxing(&mut program);
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "took {elapsed:?}"
    );
    assert!(
        program.functions[..N]
            .iter()
            .all(|function| function.ret == Repr::TObj)
    );
}

#[test]
fn an_apply_in_ir_text_boxes_its_argument_and_rebinds_its_result() {
    // 結果を `tobj` の新しい変数で受け、元の変数を直後の `unbox` で定義する。使う所の変数は書き換えない
    let text = "\
fn f(c.0: tobj, n.1: int) -> int {
  let r.2: int = apply c.0(n.1)
  let s.3: int = extern Prelude.+(r.2, 1)
  return s.3
}
";
    insta::assert_snapshot!(boxing_text(text), @"
    fn f(c.0: tobj, n.1: int) -> int {
      let n.4: tobj = box n.1
      let r.5: tobj = apply c.0(n.4)
      let r.2: int = unbox r.5
      let s.3: int = extern Prelude.+(r.2, 1)
      return s.3
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
    // `put` の `int` の引数へ `tobj` の n.2 を `unbox` して渡し、`get` の `int` の結果を受け直して `box` する
    let text = "effect Counter { get(unit) -> int, put(int) -> unit }\nyields fn f(c.0: tobj, e.1: obj, n.2: tobj) -> tobj {\n  let u.3: unit = clause Counter.put c.0(e.1, n.2)\n  let c.4: tobj = prim ev_clause[Counter.get](e.1)\n  let m.5: tobj = clause Counter.get c.4(e.1, ())\n  return m.5\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"
    yields fn f(c.0: tobj, e.1: obj, n.2: tobj) -> tobj {
      let n.6: int = unbox n.2
      let u.3: unit = clause Counter.put c.0(e.1, n.6)
      let c.4: tobj = prim ev_clause[Counter.get](e.1)
      let m.7: int = clause Counter.get c.4(e.1, ())
      let m.5: tobj = box m.7
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
    // `never` の操作の節の呼び出しは戻らないので、`int` の束縛を受け直さず、`return` にも変換を入れない。T3 も見ない
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
    // `int` の束縛を `tobj` の位置 (`apply` の引数と `tobj` の `ret` の `return`) へ渡しても `box` しない。使いには制御が
    // 届かないからである
    let text = "effect Fail { never fail(tobj) -> never }\nyields fn f(c.0: tobj, e.1: obj, s.2: obj, k.3: tobj) -> tobj {\n  let r.4: int = clause Fail.fail c.0(e.1, s.2)\n  let t.5: tobj = apply k.3(r.4)\n  return t.5\n}\nyields fn g(c.0: tobj, e.1: obj, s.2: obj) -> tobj {\n  let r.3: int = clause Fail.fail c.0(e.1, s.2)\n  return r.3\n}\n";
    let shown = boxing_text(text);
    insta::assert_snapshot!(function(&shown, "f"), @"
    yields fn f(c.0: tobj, e.1: obj, s.2: obj, k.3: tobj) -> tobj {
      let r.4: int = clause Fail.fail c.0(e.1, s.2)
      let t.5: tobj = apply k.3(r.4)
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

#[test]
fn t3_takes_the_result_of_a_tail_clause_from_its_operation() {
    // `Counter.get` の `int` の結果は `int` の `ret` と互換なので上げない。`State.get` の `tobj` の結果は互換でないので、
    // `ret` を `tobj` に上げる
    let text = "effect Counter { get(unit) -> int, put(int) -> unit }\neffect State { get(unit) -> tobj, put(tobj) -> unit }\nyields fn f(c.0: tobj, e.1: obj) -> int {\n  let m.2: int = clause Counter.get c.0(e.1, ())\n  return m.2\n}\nyields fn g(c.0: tobj, e.1: obj) -> int {\n  let m.2: int = clause State.get c.0(e.1, ())\n  return m.2\n}\n";
    let shown = boxing_text(text);
    insta::assert_snapshot!(function(&shown, "f"), @"
    yields fn f(c.0: tobj, e.1: obj) -> int {
      let m.2: int = clause Counter.get c.0(e.1, ())
      return m.2
    }
    ");
    insta::assert_snapshot!(function(&shown, "g"), @"
    yields fn g(c.0: tobj, e.1: obj) -> tobj {
      let m.3: tobj = clause State.get c.0(e.1, ())
      let m.2: int = unbox m.3
      return m.3
    }
    ");
}

#[test]
fn a_boxed_wrapper_copies_the_marks_of_its_function() {
    // `f$boxed` の引数と戻り値は `f` と同じ値なので、`f` の印を写す。`box` と `unbox` で足す変数はスカラーなので
    // `unr` である (docs/spec/lir.md の「box の挿入」)
    let text = "\
fn f(n.0: int, h.1: obj lin) -> obj lin {
  return h.1
}
fn main(h.0: obj lin) -> obj lin {
  let g.1: tobj = closure f(1)
  let r.2: tobj poly = apply g.1(h.0)
  return r.2
}
";
    insta::assert_snapshot!(function(&boxing_text(text), "f$boxed"), @"
    fn f$boxed(n.0: tobj, h.1: obj lin) -> obj lin {
      let n.3: int = unbox n.0
      let t.2: obj lin = call f(n.3, h.1)
      return t.2
    }
    ");
}
