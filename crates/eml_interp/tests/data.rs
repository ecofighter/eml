//! Core IR のテキストで、`data` の値の確保と、`Switch` の枝による分解を確かめる (docs/spec/core-ir.md)。共有された
//! 値の分解や、boxed な変数に入った引数のないコンストラクタは、ソースの `match` からは狙って作りにくいので、ここで
//! 書く。

use eml_interp::{Fault, RuntimeError};

use crate::common::{run_core, run_core_unverified};

const UNIQUE: &str = r#"
fn main() {
  let s0^ = const "field"
  let d1^ = con #1(s0)
  switch d1 {
    #0 ->
      return ()
    #1(x2^) ->
      let o3 = extern Prelude.println(x2)
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
      let o3 = extern Prelude.println(x2)
      decref d1
      return o3
  }
}
"#;

#[test]
fn a_unique_value_is_unpacked_by_taking_its_fields() {
    let (stdout, result) = run_core(UNIQUE);
    assert_eq!((stdout.as_str(), result), ("field\n", Ok(())));
}

#[test]
fn a_shared_value_is_unpacked_by_copying_its_fields() {
    let (stdout, result) = run_core(SHARED);
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
      let o3 = extern Prelude.println(s2)
      return o3
    #1(x1^) ->
      decref x1
      return ()
  }
}
"#;
    let (stdout, result) = run_core(text);
    assert_eq!((stdout.as_str(), result), ("none\n", Ok(())));
}

#[test]
fn a_case_with_a_different_number_of_fields_is_an_internal_error() {
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
    let (_, result) = run_core_unverified(text);
    assert_eq!(
        result,
        Err(RuntimeError::Fault {
            fault: Fault::Internal(
                "a switch case binds a different number of fields than the value has"
            ),
            function: "main".to_string(),
        })
    );
}

/// 文字列のリテラルの case に一致する値と、`default` に進む値。どちらも `Switch` が文字列を1回だけ手放す。
const STRING_SWITCH: &str = r#"
fn main() {
  let s0^ = const "a"
  let n1 = call pick(s0)
  let s2^ = const "b"
  let n3 = call pick(s2) [n1]
  let t4 = extern Prelude.+(n1, n3)
  let t5^ = extern Prelude.show_int(t4)
  let o6 = extern Prelude.println(t5)
  return o6
}
fn pick(s0^) {
  switch s0 {
    "a" ->
      return 1
    _ ->
      return 2
  }
}
"#;

#[test]
fn a_string_switch_releases_the_string_on_every_path() {
    let (out, result) = run_core(STRING_SWITCH);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "3\n");
}

/// フィールドを持つ値が `default` に進む。`Switch` は値を分解せずに手放す。
const DEFAULT_WITH_FIELDS: &str = r#"
fn main() {
  let s0^ = const "field"
  let d1^ = con #1(s0)
  switch d1 {
    #0 ->
      return ()
    _ ->
      let s2^ = const "default"
      let o3 = extern Prelude.println(s2)
      return o3
  }
}
"#;

#[test]
fn a_value_with_fields_that_goes_to_the_default_is_released() {
    let (out, result) = run_core(DEFAULT_WITH_FIELDS);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "default\n");
}

#[test]
fn an_int_switch_goes_to_the_matching_case() {
    let text = r#"
fn main() {
  switch 2 {
    1 ->
      let s0^ = const "one"
      let o1 = extern Prelude.println(s0)
      return o1
    2 ->
      let s2^ = const "two"
      let o3 = extern Prelude.println(s2)
      return o3
    _ ->
      return ()
  }
}
"#;
    let (out, result) = run_core(text);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(out, "two\n");
}
