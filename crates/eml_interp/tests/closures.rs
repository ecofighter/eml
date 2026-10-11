//! LIR のテキストで、クロージャの eval/apply と、handler をまたぐ再開を確かめる (docs/spec/core-ir.md)。関数の
//! 値として使う関数は一様なので、`Int` は `box` して渡し、結果は `unbox` して受ける。

use crate::common::run_lir;

/// `first` は2つの引数のうち最初のものを返す。
const FIRST: &str = "\
fn first(a.0: tobj, b.1: tobj) -> tobj {
  decref b.1
  return a.0
}
";

/// `make` は `first` に1つめの引数だけを渡したクロージャを返す。
const MAKE: &str = "\
fn make(x.0: tobj) -> tobj {
  let c.1: tobj = closure first(x.0)
  return c.1
}
";

#[test]
fn a_partial_application_waits_for_the_rest_of_the_arguments() {
    let text = format!(
        "\
fn main() -> unit {{
  let a.0: tobj = box 10
  let d.1: tobj = apply &first(a.0)
  let b.2: tobj = box 20
  let q.3: tobj = apply d.1(b.2)
  let r.4: int = unbox q.3
  decref q.3
  let s.5: obj = extern \"Prelude.Show Int.show\"(r.4)
  let t.6: unit = extern Prelude.println(s.5)
  return t.6
}}
{FIRST}"
    );
    assert_eq!(run_lir(&text), ("10\n".to_string(), Ok(())));
}

#[test]
fn a_returned_function_can_still_wait_for_more_arguments() {
    let text = "\
fn main() -> unit {
  let a.0: tobj = box 5
  let b.1: tobj = box 6
  let r.2: tobj = apply &make(a.0, b.1)
  let c.3: tobj = box 7
  let q.4: tobj = apply r.2(c.3)
  let s.5: int = unbox q.4
  decref q.4
  let t.6: obj = extern \"Prelude.Show Int.show\"(s.5)
  let u.7: unit = extern Prelude.println(t.6)
  return u.7
}
fn first3(a.0: tobj, b.1: tobj, c.2: tobj) -> tobj {
  decref b.1
  decref c.2
  return a.0
}
fn make(x.0: tobj) -> tobj {
  let c.1: tobj = closure first3(x.0)
  return c.1
}
";
    assert_eq!(run_lir(text), ("5\n".to_string(), Ok(())));
}

#[test]
fn extra_arguments_are_applied_to_the_returned_function() {
    let text = format!(
        "\
fn main() -> unit {{
  let a.0: tobj = box 5
  let b.1: tobj = box 6
  let q.2: tobj = apply &make(a.0, b.1)
  let r.3: int = unbox q.2
  decref q.2
  let s.4: obj = extern \"Prelude.Show Int.show\"(r.3)
  let t.5: unit = extern Prelude.println(s.4)
  return t.5
}}
{FIRST}{MAKE}"
    );
    assert_eq!(run_lir(&text), ("5\n".to_string(), Ok(())));
}

#[test]
fn a_shared_closure_keeps_its_captured_values() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"a\"
  let c.1: tobj = closure first(s.0)
  dup c.1
  let n.2: tobj = box 1
  let r.3: obj = apply c.1(n.2) save [c.1]
  let t.4: unit = extern Prelude.println(r.3)
  let m.5: tobj = box 2
  let r.6: obj = apply c.1(m.5)
  let t.7: unit = extern Prelude.println(r.6)
  return t.7
}
fn first(a.0: obj, b.1: tobj) -> obj {
  decref b.1
  return a.0
}
";
    assert_eq!(run_lir(text), ("a\na\n".to_string(), Ok(())));
}

#[test]
fn a_function_value_is_applied_like_a_closure_without_arguments() {
    // 変数に入った関数の値で、等しい、足りない、余るの3つの場合を確かめる (docs/spec/core-ir.md の eval/apply)
    let text = format!(
        "\
fn main() -> unit {{
  tail call each(&first, &make)
}}
fn each(f.0: tobj, m.1: tobj) -> unit {{
  let a.2: tobj = box 10
  let d.3: tobj = apply f.0(a.2) save [m.1]
  let b.4: tobj = box 20
  let q.5: tobj = apply d.3(b.4) save [m.1]
  let r.6: int = unbox q.5
  decref q.5
  let s.7: obj = extern \"Prelude.Show Int.show\"(r.6)
  let t.8: unit = extern Prelude.println(s.7)
  let c.9: tobj = box 5
  let e.10: tobj = box 6
  let p.11: tobj = apply m.1(c.9, e.10)
  let r.12: int = unbox p.11
  decref p.11
  let s.13: obj = extern \"Prelude.Show Int.show\"(r.12)
  let t.14: unit = extern Prelude.println(s.13)
  return t.14
}}
{FIRST}{MAKE}"
    );
    assert_eq!(run_lir(&text), ("10\n5\n".to_string(), Ok(())));
}

#[test]
fn a_function_value_needs_no_reference_counting() {
    let text = format!(
        "\
layout (,) {{ (,)(tobj, tobj) }}
fn main() -> unit {{
  tail call pair(&first)
}}
fn pair(f.0: tobj) -> unit {{
  dup f.0
  let d.1: obj = con (,) #0(f.0, f.0)
  decref d.1
  let s.2: obj = const \"ok\"
  let t.3: unit = extern Prelude.println(s.2)
  return t.3
}}
{FIRST}"
    );
    assert_eq!(run_lir(&text), ("ok\n".to_string(), Ok(())));
}

/// `ir/multi_resume.lir` は、次のプログラムを Perceus まで通した IR である。`Choose` の節が `k` を2回再開し、
/// 内側の `Ask` の handle の `prompt$` は、再開のたびに `return` の節を通って戻る。LIR のテキストにはコメントを
/// 書けないので、IR の説明はここに書く。
///
/// ```text
/// effect Choose where
///   multi choose : Unit -> Int
///
/// effect Ask where
///   ask : Unit -> Int
///
/// main : Unit -> <IO> Unit
/// main () =
///   let r = handle (handle choose () with
///                     | ask () k -> k 0
///                     | return x -> x + 100) with
///             | choose () k -> k 1 + k 2
///   println (show r)
/// ```
#[test]
fn an_inner_handle_returns_through_each_resumption_of_an_outer_multi_operation() {
    let (stdout, result) = run_lir(include_str!("ir/multi_resume.lir"));
    assert_eq!(result, Ok(()));
    assert_eq!(stdout, "203\n");
}
