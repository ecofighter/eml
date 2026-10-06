//! Core IR のテキストで、クロージャの eval/apply を確かめる (docs/spec/core-ir.md)。

fn run_program(text: &str) -> String {
    let program = eml_core_ir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    let (stdout, result) = eml_test_support::execute(program, true);
    result.unwrap();
    stdout
}

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
    assert_eq!(run_program(text), "10\n");
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
    assert_eq!(run_program(text), "5\n");
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
    assert_eq!(run_program(text), "5\n");
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
    assert_eq!(run_program(text), "a\na\n");
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
    assert_eq!(run_program(text), "10\n5\n");
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
    assert_eq!(run_program(text), "ok\n");
}
