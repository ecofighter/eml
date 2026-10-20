//! contract のパス (docs/spec/lir.md の「縮約」)。使われない純粋な `let` を消すことと、すべてのブロックに
//! 末尾呼び出しの規則を当てることを、IR のテキストで確かめる。ソースから縮約までを通すテストは `eml_core_ir` の
//! tests/contract.rs にある。

use eml_lir::{contract, parse, pretty, verify_scopes};

/// 入力と出力が、どちらも scope の段の verifier を通ることも確かめる。
fn contract_text(text: &str) -> String {
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    if let Err(error) = verify_scopes(&program) {
        panic!("the input must verify: {error}\n{text}");
    }
    contract(&mut program);
    let shown = pretty(&program);
    if let Err(error) = verify_scopes(&program) {
        panic!("the output must verify: {error}\n{shown}");
    }
    shown
}

#[test]
fn unused_bindings_that_cannot_fail_are_removed() {
    let text = "\
layout Pair { Pair(int, int) }
fn f(x.0: int) -> int {
  let p.1: obj = con Pair #0(x.0, x.0)
  let s.2: obj = const \"unused\"
  let c.3: tobj = closure g(())
  return 1
}
fn g(x.0: unit, y.1: tobj) -> tobj {
  return y.1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    layout Pair { Pair(int, int) }
    fn f(x.0: int) -> int {
      return 1
    }
    fn g(x.0: unit, y.1: tobj) -> tobj {
      return y.1
    }
    ");
}

#[test]
fn unused_bindings_that_can_fail_or_act_are_kept() {
    // `/` はゼロ除算で止まりうる。extern のエフェクト、呼び出し、`drop` も、使われなくても消さない
    let text = "\
fn f(x.0: int, s.1: obj, t.2: obj) -> int {
  let q.3: int = extern \"Prelude.Div Int./\"(x.0, 0)
  let u.4: unit = extern Prelude.println(s.1)
  let r.5: int = call g(x.0)
  let d.6: unit = drop t.2
  return 1
}
fn g(x.0: int) -> int {
  return x.0
}
";
    assert_eq!(contract_text(text), text);
}

#[test]
fn unused_pure_extern_bindings_are_removed() {
    // 行が `Pure` の extern は止まらないので、使われなければ消す
    let text = "\
fn f(x.0: int, s.1: obj) -> int {
  let t.2: obj = extern \"Prelude.Show Int.show\"(x.0)
  let b.3: enum = extern \"Prelude.Ord Int.<\"(x.0, 0)
  let u.4: obj = extern Prelude.++(s.1, t.2)
  return 1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(x.0: int, s.1: obj) -> int {
      return 1
    }
    ");
}

#[test]
fn unused_boxes_and_unboxes_are_removed() {
    // `box` を消すと確保が1つ減るだけで、`unbox` は読むだけなので、どちらも純粋である
    let text = "\
fn f(n.0: int, b.1: tobj) -> int {
  let c.2: tobj = box n.0
  let m.3: int = unbox b.1
  return n.0
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(n.0: int, b.1: tobj) -> int {
      return n.0
    }
    ");
}

#[test]
fn bindings_used_only_by_removed_bindings_go_in_the_same_pass() {
    // `q.4` を消すと `p.3` が、`p.3` を消すと `s.2` が使われなくなる。後ろから1回たどるだけで、ブロックをまたいで
    // すべて消える
    let text = "\
layout Prelude.Bool { False, True }
layout Pair { Pair(tobj, int) }
layout Either { Left(tobj), Right(tobj) }
fn f(x.0: int, c.1: enum) -> int {
  let s.2: obj = extern \"Prelude.Show Int.show\"(x.0)
  let p.3: obj = con Pair #0(s.2, x.0)
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  let q.4: obj = con Either #1(p.3)
  return 1
b2:
  return 2
}
";
    insta::assert_snapshot!(contract_text(text), @"
    layout Prelude.Bool { False, True }
    layout Pair { Pair(tobj, int) }
    layout Either { Left(tobj), Right(tobj) }
    fn f(x.0: int, c.1: enum) -> int {
      switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      return 1
    b2:
      return 2
    }
    ");
}

#[test]
fn a_call_returned_at_the_end_of_an_unchanged_block_becomes_a_tail_call() {
    // `f` は使われない `let` を消してから、`h` は何も消さずに、どちらも末尾呼び出しになる。translate は末尾呼び出しを
    // 作らないので、縮約は文を消したかに関わらずすべてのブロックに規則を当てる
    let text = "\
fn f(x.0: int) -> int {
  let r.1: int = call g(x.0)
  let s.2: obj = const \"unused\"
  return r.1
}
fn g(x.0: int) -> int {
  return x.0
}
fn h(x.0: int) -> int {
  let r.1: int = call g(x.0)
  return r.1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(x.0: int) -> int {
      tail call g(x.0)
    }
    fn g(x.0: int) -> int {
      return x.0
    }
    fn h(x.0: int) -> int {
      tail call g(x.0)
    }
    ");
}

#[test]
fn every_kind_of_call_returned_at_the_end_becomes_a_tail_call() {
    // 直接の呼び出し、関数値の適用、節の呼び出しのどれも末尾呼び出しになる (docs/spec/lir.md の「縮約」)。
    // 結果を返さない呼び出しは残す
    let text = "\
effect Ask { ask(tobj) -> tobj }
fn by_call(x.0: int) -> int {
  let r.1: int = call by_call(x.0)
  return r.1
}
fn by_apply(f.0: tobj) -> tobj {
  let r.1: tobj = apply f.0((): unit)
  return r.1
}
yields fn by_clause(c.0: tobj, e.1: obj, s.2: obj) -> tobj {
  let r.3: tobj = clause Ask.ask c.0(e.1, s.2)
  return r.3
}
fn not_returned(x.0: int) -> int {
  let r.1: int = call by_call(x.0)
  return x.0
}
";
    insta::assert_snapshot!(contract_text(text), @"
    effect Ask { ask(tobj) -> tobj }
    fn by_call(x.0: int) -> int {
      tail call by_call(x.0)
    }
    fn by_apply(f.0: tobj) -> tobj {
      tail apply f.0((): unit)
    }
    yields fn by_clause(c.0: tobj, e.1: obj, s.2: obj) -> tobj {
      tail clause Ask.ask c.0(e.1, s.2)
    }
    fn not_returned(x.0: int) -> int {
      let r.1: int = call by_call(x.0)
      return x.0
    }
    ");
}

#[test]
fn a_call_whose_result_is_not_compatible_with_the_ret_is_not_a_tail_call() {
    // `g` の `unit` は `r.0` の `tobj` と、`r.0` は `f` の `obj` と互換である。互換は推移的でないので、`unit` と `obj`
    // を直接つなぐ末尾呼び出しにはしない
    let text = "\
fn f() -> obj {
  let r.0: tobj = call g()
  return r.0
}
fn g() -> unit {
  return ()
}
";
    assert_eq!(contract_text(text), text);
}

#[test]
fn a_clause_call_returned_at_the_end_becomes_a_tail_call() {
    // 節の呼び出しの結果は操作の `ret` である。`Counter.get` の `int` は `f` の `int` の `ret` と互換なので末尾呼び出し
    // になる。`g` では `Counter.put` の `unit` が `u.3` の `tobj` を通して `obj` の `ret` につながるが、`unit` と `obj`
    // は互換でないので末尾呼び出しにならない
    let text = "\
effect Counter { get(unit) -> int, put(int) -> unit }
yields fn f(c.0: tobj, e.1: obj) -> int {
  let m.2: int = clause Counter.get c.0(e.1, ())
  return m.2
}
yields fn g(c.0: tobj, e.1: obj, n.2: int) -> obj {
  let u.3: tobj = clause Counter.put c.0(e.1, n.2)
  return u.3
}
";
    insta::assert_snapshot!(contract_text(text), @"
    effect Counter { get(unit) -> int, put(int) -> unit }
    yields fn f(c.0: tobj, e.1: obj) -> int {
      tail clause Counter.get c.0(e.1, ())
    }
    yields fn g(c.0: tobj, e.1: obj, n.2: int) -> obj {
      let u.3: tobj = clause Counter.put c.0(e.1, n.2)
      return u.3
    }
    ");
}

#[test]
fn a_never_clause_returned_at_the_end_becomes_a_tail_call_in_any_function() {
    // `never` の操作の節の呼び出しは戻らないので、結果はどの `ret` とも互換である
    let text = "\
effect Fail { never fail(tobj) -> never }
yields fn f(c.0: tobj, e.1: obj, s.2: obj) -> int {
  let r.3: int = clause Fail.fail c.0(e.1, s.2)
  return r.3
}
";
    insta::assert_snapshot!(contract_text(text), @"
    effect Fail { never fail(tobj) -> never }
    yields fn f(c.0: tobj, e.1: obj, s.2: obj) -> int {
      tail clause Fail.fail c.0(e.1, s.2)
    }
    ");
}

#[test]
fn a_call_whose_result_mark_is_not_compatible_with_the_ret_mark_is_not_a_tail_call() {
    // `h` の `lin` は `r` の `poly` と、`r` は `f` の `unr` と互換である。印の互換も推移的でないので、`lin` と `unr` を
    // 直接つなぐ末尾呼び出しにはしない。`apply` の結果はいつも `poly` なので、どの戻り値の印とも互換である
    // (docs/spec/lir.md の「縮約」と「Kind の印」)
    let text = "\
effect L { take(tobj lin) -> tobj lin }
fn f(s.0: obj) -> obj {
  let r.1: obj poly = call h(s.0)
  return r.1
}
yields fn g(c.0: tobj, e.1: obj, x.2: tobj lin) -> tobj {
  let r.3: tobj poly = clause L.take c.0(e.1, x.2)
  return r.3
}
fn h(s.0: obj) -> obj lin {
  let f.1: obj lin = extern Std.Fs.open(s.0)
  return f.1
}
fn k(c.0: tobj) -> tobj lin {
  let r.1: tobj poly = apply c.0((): unit)
  return r.1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    effect L { take(tobj lin) -> tobj lin }
    fn f(s.0: obj) -> obj {
      let r.1: obj poly = call h(s.0)
      return r.1
    }
    yields fn g(c.0: tobj, e.1: obj, x.2: tobj lin) -> tobj {
      let r.3: tobj poly = clause L.take c.0(e.1, x.2)
      return r.3
    }
    fn h(s.0: obj) -> obj lin {
      let f.1: obj lin = extern Std.Fs.open(s.0)
      return f.1
    }
    fn k(c.0: tobj) -> tobj lin {
      tail apply c.0((): unit)
    }
    ");
}

/// 根を読むだけの `root_get` は、結果が使われなければ消す。根に置く `root_set` は消さない。
#[test]
fn an_unused_root_get_is_removed_and_a_root_set_is_kept() {
    let text = "\
fn f(x.0: tobj) -> unit {
  let y.1: tobj = prim root_get[0]()
  let u.2: unit = prim root_set[1](x.0)
  return ()
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(x.0: tobj) -> unit {
      let u.2: unit = prim root_set[1](x.0)
      return ()
    }
    ");
}
