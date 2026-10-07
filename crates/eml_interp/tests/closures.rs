//! Core IR のテキストで、クロージャの eval/apply を確かめる (docs/spec/core-ir.md)。

use crate::common::run_core;

#[test]
fn a_partial_application_waits_for_the_rest_of_the_arguments() {
    let text = r#"
fn main() {
  let c1^ = &first
  let d2^ = apply c1(10)
  let r3 = apply d2(20)
  let s4^ = prim show_int(r3)
  let t5 = perform println(s4)
  return t5
}
fn first(a0, b1) {
  return a0
}
"#;
    assert_eq!(run_core(text, true), ("10\n".to_string(), Ok(())));
}

#[test]
fn a_returned_function_can_still_wait_for_more_arguments() {
    let text = r#"
fn main() {
  let m1^ = &make
  let r2^ = apply m1(5, 6)
  let s3 = apply r2(7)
  let t4^ = prim show_int(s3)
  let u5 = perform println(t4)
  return u5
}
fn first3(a0, b1, c2) {
  return a0
}
fn make(x0) {
  let c1^ = closure first3(x0)
  return c1
}
"#;
    assert_eq!(run_core(text, true), ("5\n".to_string(), Ok(())));
}

#[test]
fn extra_arguments_are_applied_to_the_returned_function() {
    let text = r#"
fn main() {
  let m1^ = &make
  let r2 = apply m1(5, 6)
  let s3^ = prim show_int(r2)
  let t4 = perform println(s3)
  return t4
}
fn first(a0, b1) {
  return a0
}
fn make(x0) {
  let c1^ = closure first(x0)
  return c1
}
"#;
    assert_eq!(run_core(text, true), ("5\n".to_string(), Ok(())));
}

#[test]
fn a_shared_closure_keeps_its_captured_values() {
    let text = r#"
fn main() {
  let s1^ = const "a"
  let c2^ = closure first(s1)
  dup c2
  let r3^ = apply c2(1) [c2]
  let t4 = perform println(r3)
  let r5^ = apply c2(2)
  let t6 = perform println(r5)
  return t6
}
fn first(a0^, b1) {
  return a0
}
"#;
    assert_eq!(run_core(text, true), ("a\na\n".to_string(), Ok(())));
}

#[test]
fn a_function_value_is_applied_like_a_closure_without_arguments() {
    // 等しい、足りない、余るの3つの場合 (docs/spec/core-ir.md の eval/apply)
    let text = r#"
fn main() {
  let f1^ = &first
  let d2^ = apply f1(10)
  let r3 = apply d2(20)
  let s4^ = prim show_int(r3)
  let t5 = perform println(s4)
  let m6^ = &make
  let r7 = apply m6(5, 6)
  let s8^ = prim show_int(r7)
  let t9 = perform println(s8)
  return t9
}
fn first(a0, b1) {
  return a0
}
fn make(x0) {
  let c1^ = closure first(x0)
  return c1
}
"#;
    assert_eq!(run_core(text, true), ("10\n5\n".to_string(), Ok(())));
}

#[test]
fn a_function_value_needs_no_reference_counting() {
    let text = r#"
fn main() {
  let f1^ = &first
  dup f1
  let d2^ = con #0(f1, f1)
  decref d2
  let s3^ = const "ok"
  let t4 = perform println(s3)
  return t4
}
fn first(a0, b1) {
  return a0
}
"#;
    assert_eq!(run_core(text, true), ("ok\n".to_string(), Ok(())));
}

#[test]
fn an_inner_handle_returns_through_each_resumption_of_an_outer_multi_operation() {
    // `Choose` の節が `k` を2回再開する。区間には内側の `Ask` の handler フレームが入り、写される
    let text = r#"
effect Choose { choose/1 }
effect Ask { ask/1 }
fn main() {
  let t0 = handle Choose(&outer_body, ()) {choose: &choose} return &outer_ret
  let s1^ = prim show_int(t0)
  let t2 = perform println(s1)
  return t2
}
fn outer_body(u0) {
  tailcall handle Ask(&inner_body, ()) {ask: &ask} return &inner_ret
}
fn inner_body(u0) {
  tailcall perform Choose.choose(())
}
fn inner_ret(x0, s1) {
  let t2 = prim +(x0, 100)
  return t2
}
fn ask(u0, k1^, s2) {
  tailcall resume k1(0, s2)
}
fn choose(u0, k1^, s2) {
  dup k1
  let a3 = resume k1(1, s2) [k1]
  let b4 = resume k1(2, ()) [a3]
  let t5 = prim +(a3, b4)
  return t5
}
fn outer_ret(x0, s1) {
  return x0
}
"#;
    assert_eq!(run_core(text, true), ("203\n".to_string(), Ok(())));
}
