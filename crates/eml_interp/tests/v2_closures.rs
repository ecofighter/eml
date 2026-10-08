//! v2 の Core IR のテキストで、クロージャの eval/apply と、handler をまたぐ再開を確かめる (docs/spec/core-ir.md)。

use crate::common::run_v2;

/// `first` は2つの引数のうち最初のものを返す。
const FIRST: &str = "\
fn first(a.0: int, b.1: int) -> int {
  return a.0
}
";

/// `make` は `first` に1つめの引数だけを渡したクロージャを返す。
const MAKE: &str = "\
fn make(x.0: int) -> tobj {
  let c.1: tobj = closure first(x.0)
  return c.1
}
";

#[test]
fn a_partial_application_waits_for_the_rest_of_the_arguments() {
    let text = format!(
        "\
fn main() -> unit {{
  let d.0: tobj = apply &first(10)
  let r.1: int = apply d.0(20)
  let s.2: obj = extern Prelude.show_int(r.1)
  let t.3: unit = extern Prelude.println(s.2)
  return t.3
}}
{FIRST}"
    );
    assert_eq!(run_v2(&text), ("10\n".to_string(), Ok(())));
}

#[test]
fn a_returned_function_can_still_wait_for_more_arguments() {
    let text = "\
fn main() -> unit {
  let r.0: tobj = apply &make(5, 6)
  let s.1: int = apply r.0(7)
  let t.2: obj = extern Prelude.show_int(s.1)
  let u.3: unit = extern Prelude.println(t.2)
  return u.3
}
fn first3(a.0: int, b.1: int, c.2: int) -> int {
  return a.0
}
fn make(x.0: int) -> tobj {
  let c.1: tobj = closure first3(x.0)
  return c.1
}
";
    assert_eq!(run_v2(text), ("5\n".to_string(), Ok(())));
}

#[test]
fn extra_arguments_are_applied_to_the_returned_function() {
    let text = format!(
        "\
fn main() -> unit {{
  let r.0: int = apply &make(5, 6)
  let s.1: obj = extern Prelude.show_int(r.0)
  let t.2: unit = extern Prelude.println(s.1)
  return t.2
}}
{FIRST}{MAKE}"
    );
    assert_eq!(run_v2(&text), ("5\n".to_string(), Ok(())));
}

#[test]
fn a_shared_closure_keeps_its_captured_values() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"a\"
  let c.1: tobj = closure first(s.0)
  dup c.1
  let r.2: obj = apply c.1(1) save [c.1]
  let t.3: unit = extern Prelude.println(r.2)
  let r.4: obj = apply c.1(2)
  let t.5: unit = extern Prelude.println(r.4)
  return t.5
}
fn first(a.0: obj, b.1: int) -> obj {
  return a.0
}
";
    assert_eq!(run_v2(text), ("a\na\n".to_string(), Ok(())));
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
  let d.2: tobj = apply f.0(10) save [m.1]
  let r.3: int = apply d.2(20) save [m.1]
  let s.4: obj = extern Prelude.show_int(r.3)
  let t.5: unit = extern Prelude.println(s.4)
  let r.6: int = apply m.1(5, 6)
  let s.7: obj = extern Prelude.show_int(r.6)
  let t.8: unit = extern Prelude.println(s.7)
  return t.8
}}
{FIRST}{MAKE}"
    );
    assert_eq!(run_v2(&text), ("10\n5\n".to_string(), Ok(())));
}

#[test]
fn a_function_value_needs_no_reference_counting() {
    let text = format!(
        "\
fn main() -> unit {{
  tail call pair(&first)
}}
fn pair(f.0: tobj) -> unit {{
  dup f.0
  let d.1: obj = con #0(f.0, f.0)
  decref d.1
  let s.2: obj = const \"ok\"
  let t.3: unit = extern Prelude.println(s.2)
  return t.3
}}
{FIRST}"
    );
    assert_eq!(run_v2(&text), ("ok\n".to_string(), Ok(())));
}

#[test]
fn an_inner_handle_returns_through_each_resumption_of_an_outer_multi_operation() {
    // `Choose` の節が `k` を2回再開する。区間には内側の `Ask` の handler フレームが入り、写される
    let text = "\
effect Choose { choose/1 }
effect Ask { ask/1 }
fn main() -> unit {
  let t.0: int = handle Choose((), &outer_body) { choose: &choose } return &outer_ret
  let s.1: obj = extern Prelude.show_int(t.0)
  let t.2: unit = extern Prelude.println(s.1)
  return t.2
}
fn outer_body(u.0: unit) -> int {
  tail handle Ask((), &inner_body) { ask: &ask } return &inner_ret
}
fn inner_body(u.0: unit) -> int {
  tail perform Choose.choose(())
}
fn inner_ret(x.0: int, s.1: unit) -> int {
  let t.2: int = extern Prelude.+(x.0, 100)
  return t.2
}
fn ask(u.0: unit, k.1: obj, s.2: unit) -> int {
  tail resume k.1(0, s.2)
}
fn choose(u.0: unit, k.1: obj, s.2: unit) -> int {
  dup k.1
  let a.3: int = resume k.1(1, s.2) save [k.1]
  let b.4: int = resume k.1(2, ()) save [a.3]
  let t.5: int = extern Prelude.+(a.3, b.4)
  return t.5
}
fn outer_ret(x.0: int, s.1: unit) -> int {
  return x.0
}
";
    assert_eq!(run_v2(text), ("203\n".to_string(), Ok(())));
}
