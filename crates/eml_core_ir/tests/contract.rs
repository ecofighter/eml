//! contract のパス (docs/spec/core-ir.md の「パス」)。使われない純粋な `let` を消すことと、消したブロックの末尾に
//! 末尾呼び出しの規則をもう一度当てることを、IR のテキストで確かめる。

use eml_core_ir::{contract, parse, pretty, tail_call, verify_scopes};

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
fn f(x.0: int) -> int {
  let p.1: obj = con #0(x.0, x.0)
  let s.2: obj = const \"unused\"
  let c.3: tobj = closure g(x.0)
  return 1
}
fn g(x.0: int, y.1: int) -> int {
  return y.1
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(x.0: int) -> int {
      return 1
    }
    fn g(x.0: int, y.1: int) -> int {
      return y.1
    }
    ");
}

#[test]
fn unused_bindings_that_can_fail_or_act_are_kept() {
    // `/` はゼロ除算で止まりうる。extern のエフェクト、呼び出し、`drop` も、使われなくても消さない
    let text = "\
fn f(x.0: int, s.1: obj, t.2: obj) -> int {
  let q.3: int = extern Prelude./(x.0, 0)
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
  let t.2: obj = extern Prelude.show_int(x.0)
  let b.3: enum = extern Prelude.<(x.0, 0)
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
fn bindings_used_only_by_removed_bindings_go_in_the_same_pass() {
    // `q.4` を消すと `p.3` が、`p.3` を消すと `s.2` が使われなくなる。後ろから1回たどるだけで、ブロックをまたいで
    // すべて消える
    let text = "\
fn f(x.0: int, c.1: enum) -> int {
  let s.2: obj = extern Prelude.show_int(x.0)
  let p.3: obj = con #0(s.2, x.0)
  switch c.1 { #0 -> b1, #1 -> b2 }
b1:
  let q.4: obj = con #1(p.3)
  return 1
b2:
  return 2
}
";
    insta::assert_snapshot!(contract_text(text), @"
    fn f(x.0: int, c.1: enum) -> int {
      switch c.1 { #0 -> b1, #1 -> b2 }
    b1:
      return 1
    b2:
      return 2
    }
    ");
}

#[test]
fn a_call_left_at_the_end_of_a_changed_block_becomes_a_tail_call() {
    // `h` のブロックは何も消えないので、規則を当て直さない。translate の `finish` が当て終えているためである
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
      let r.1: int = call g(x.0)
      return r.1
    }
    ");
}

#[test]
fn every_kind_of_call_returned_at_the_end_becomes_a_tail_call() {
    // 直接の呼び出し、関数値の適用、`perform`、`handle`、`resume` のどれも末尾呼び出しになり、`mask` はそのまま運ぶ
    // (docs/spec/core-ir.md の「Core IR」)。結果を返さない呼び出しは残す
    let text = "\
effect Ask { ask/1 }
fn by_call(x.0: int) -> int {
  let r.1: int = call by_call(x.0)
  return r.1
}
fn by_apply(f.0: tobj) -> int {
  let r.1: int = mask [Ask] apply f.0(1)
  return r.1
}
fn by_perform(s.0: obj) -> int {
  let r.1: int = perform Ask.ask(s.0)
  return r.1
}
fn by_handle() -> int {
  let r.0: int = handle Ask((), &body) { ask: &clause } return &ret
  return r.0
}
fn by_resume(k.0: tobj) -> int {
  let r.1: int = resume k.0(1, ())
  return r.1
}
fn not_returned(x.0: int) -> int {
  let r.1: int = call by_call(x.0)
  return x.0
}
fn body(u.0: unit) -> int {
  return 1
}
fn clause(s.0: obj, k.1: tobj, t.2: unit) -> int {
  return 2
}
fn ret(v.0: int, t.1: unit) -> int {
  return v.0
}
";
    let mut program = parse(text).unwrap_or_else(|error| panic!("{error}"));
    for function in &mut program.functions {
        for block in &mut function.blocks {
            tail_call(block);
        }
    }
    insta::assert_snapshot!(pretty(&program), @"
    effect Ask { ask/1 }
    fn by_call(x.0: int) -> int {
      tail call by_call(x.0)
    }
    fn by_apply(f.0: tobj) -> int {
      tail mask [Ask] apply f.0(1)
    }
    fn by_perform(s.0: obj) -> int {
      tail perform Ask.ask(s.0)
    }
    fn by_handle() -> int {
      tail handle Ask((), &body) { ask: &clause } return &ret
    }
    fn by_resume(k.0: tobj) -> int {
      tail resume k.0(1, ())
    }
    fn not_returned(x.0: int) -> int {
      let r.1: int = call by_call(x.0)
      return x.0
    }
    fn body(u.0: unit) -> int {
      return 1
    }
    fn clause(s.0: obj, k.1: tobj, t.2: unit) -> int {
      return 2
    }
    fn ret(v.0: int, t.1: unit) -> int {
      return v.0
    }
    ");
}
