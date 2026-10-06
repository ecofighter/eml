//! Core IR のテキストで、`data` の値の確保と、`Switch` の枝による分解を確かめる (docs/spec/core-ir.md)。共有された
//! 値の分解や、boxed な変数に入った引数のないコンストラクタは、ソースの `match` からは狙って作りにくいので、ここで
//! 書く。

use eml_interp::{Fault, RuntimeError};
use eml_test_support::execute;

fn run_text(text: &str) -> (String, Result<(), RuntimeError>) {
    let program = eml_core_ir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    execute(program, true)
}

const UNIQUE: &str = r#"
fn main() {
  let s0^ = const "field"
  let d1^ = con #1(s0)
  switch d1 {
    #0 ->
      return ()
    #1(x2^) ->
      let o3 = perform println(x2)
      return o3
  }
}
"#;

/// `switch` の前で `d` を複製し、両方の枝で `d` を捨てる。
const SHARED: &str = r#"
fn main() {
  let s0^ = const "field"
  let d1^ = con #1(s0)
  dup d1
  switch d1 {
    #0 ->
      decref d1
      return ()
    #1(x2^) ->
      let o3 = perform println(x2)
      decref d1
      return o3
  }
}
"#;

#[test]
fn a_unique_value_is_unpacked_by_taking_its_fields() {
    let (stdout, result) = run_text(UNIQUE);
    assert_eq!((stdout.as_str(), result), ("field\n", Ok(())));
}

#[test]
fn a_shared_value_is_unpacked_by_copying_its_fields() {
    let (stdout, result) = run_text(SHARED);
    assert_eq!((stdout.as_str(), result), ("field\n", Ok(())));
}

#[test]
fn a_boxed_variable_holding_a_tag_takes_its_arm() {
    // 引数のないコンストラクタの値は、引数を持つコンストラクタのある型の変数 (boxed) にも即値で入る
    let text = r#"
fn main() {
  let d0^ = #0
  switch d0 {
    #0 ->
      let s2^ = const "none"
      let o3 = perform println(s2)
      return o3
    #1(x1^) ->
      decref x1
      return ()
  }
}
"#;
    let (stdout, result) = run_text(text);
    assert_eq!((stdout.as_str(), result), ("none\n", Ok(())));
}

#[test]
fn an_arm_with_a_different_number_of_fields_is_an_internal_error() {
    let text = r#"
fn main() {
  let s0^ = const "field"
  let d1^ = con #1(s0)
  switch d1 {
    #0 ->
      return ()
    #1(x2^, y3^) ->
      return ()
  }
}
"#;
    let (_, result) = run_text(text);
    assert_eq!(
        result,
        Err(RuntimeError::Fault {
            fault: Fault::Internal(
                "a switch arm binds a different number of fields than the value has"
            ),
            function: "main".to_string(),
        })
    );
}
