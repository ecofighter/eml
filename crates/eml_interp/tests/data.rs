//! Core IR のテキストで、`data` の値の確保、`switch` と `unpack` による分解、`release` を確かめる (docs/spec/core-ir.md)。
//! 共有された値の分解や、`tobj` の変数に入った引数のないコンストラクタは、ソースの `match`
//! からは狙って作りにくいので、ここで書く。

use eml_interp::{Fault, RuntimeError};

use crate::common::{run_core, run_core_unverified};

const UNIQUE: &str = "\
fn main() -> unit {
  let s.0: obj = const \"field\"
  let d.1: tobj = con #1(s.0)
  switch d.1 { #0 -> b1, #1(x.2: obj) -> b2 }
b1:
  return ()
b2:
  let o.3: unit = extern Prelude.println(x.2)
  return o.3
}
";

/// `switch` の前で `d.1` を複製し、両方の行き先で `d.1` を捨てる。
const SHARED: &str = "\
fn main() -> unit {
  let s.0: obj = const \"field\"
  let d.1: tobj = con #1(s.0)
  dup d.1
  switch d.1 { #0 -> b1, #1(x.2: obj) -> b2 }
b1:
  decref d.1
  return ()
b2:
  let o.3: unit = extern Prelude.println(x.2)
  decref d.1
  return o.3
}
";

#[test]
fn a_unique_value_is_unpacked_by_taking_its_fields() {
    assert_eq!(run_core(UNIQUE), ("field\n".to_string(), Ok(())));
}

#[test]
fn a_shared_value_is_unpacked_by_copying_its_fields() {
    assert_eq!(run_core(SHARED), ("field\n".to_string(), Ok(())));
}

#[test]
fn a_tobj_variable_holding_a_tag_takes_its_case() {
    // 引数のないコンストラクタの値は、`tobj` の変数にも即値で入る
    let text = "\
fn main() -> unit {
  tail call show(#0)
}
fn show(d.0: tobj) -> unit {
  switch d.0 { #0 -> b1, #1(x.1: obj) -> b2 }
b1:
  let s.2: obj = const \"none\"
  let o.3: unit = extern Prelude.println(s.2)
  return o.3
b2:
  decref x.1
  return ()
}
";
    assert_eq!(run_core(text), ("none\n".to_string(), Ok(())));
}

#[test]
fn a_case_with_a_different_number_of_fields_is_an_internal_error() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"field\"
  let d.1: tobj = con #1(s.0)
  switch d.1 { #0 -> b1, #1(x.2: obj, y.3: obj) -> b2 }
b1:
  return ()
b2:
  return ()
}
";
    assert_eq!(
        run_core_unverified(text).1,
        Err(RuntimeError::Fault {
            fault: Fault::Internal(
                "a switch case binds a different number of fields than the value has"
            ),
            function: "main".to_string(),
            at: None,
        })
    );
}

/// 文字列のリテラルの case に一致する値と、`default` に進む値。どちらも `Switch` が文字列を1回だけ手放す。
const STRING_SWITCH: &str = "\
fn main() -> unit {
  let s.0: obj = const \"a\"
  let n.1: int = call pick(s.0)
  let s.2: obj = const \"b\"
  let n.3: int = call pick(s.2) save [n.1]
  let t.4: int = extern Prelude.+(n.1, n.3)
  let t.5: obj = extern Prelude.show_int(t.4)
  let o.6: unit = extern Prelude.println(t.5)
  return o.6
}
fn pick(s.0: obj) -> int {
  switch s.0 { \"a\" -> b1, _ -> b2 }
b1:
  return 1
b2:
  return 2
}
";

#[test]
fn a_string_switch_releases_the_string_on_every_path() {
    assert_eq!(run_core(STRING_SWITCH), ("3\n".to_string(), Ok(())));
}

#[test]
fn a_value_with_fields_that_goes_to_the_default_is_released() {
    // `default` はフィールドを束縛しないので、`Switch` は値を分解せずに手放す
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"field\"
  let d.1: tobj = con #1(s.0)
  switch d.1 { #0 -> b1, _ -> b2 }
b1:
  return ()
b2:
  let s.2: obj = const \"default\"
  let o.3: unit = extern Prelude.println(s.2)
  return o.3
}
";
    assert_eq!(run_core(text), ("default\n".to_string(), Ok(())));
}

#[test]
fn an_int_switch_goes_to_the_matching_case() {
    let text = "\
fn main() -> unit {
  switch 2 { 1 -> b1, 2 -> b2, _ -> b3 }
b1:
  let s.0: obj = const \"one\"
  let o.1: unit = extern Prelude.println(s.0)
  return o.1
b2:
  let s.2: obj = const \"two\"
  let o.3: unit = extern Prelude.println(s.2)
  return o.3
b3:
  return ()
}
";
    assert_eq!(run_core(text), ("two\n".to_string(), Ok(())));
}

/// 組を2回分解する。1回目は共有された箱からフィールドを写し、2回目は一意になった箱からフィールドを取り出す。
const UNPACK_TWICE: &str = "\
fn main() -> unit {
  let s.0: obj = const \"first\"
  let p.1: obj = con #0(s.0, 2)
  dup p.1
  unpack p.1 #0(a.2: obj, n.3: int)
  let o.4: unit = extern Prelude.println(a.2)
  unpack p.1 #0(b.5: obj, m.6: int)
  let o.7: unit = extern Prelude.println(b.5)
  return o.7
}
";

#[test]
fn an_unpack_takes_or_copies_the_fields() {
    assert_eq!(
        run_core(UNPACK_TWICE),
        ("first\nfirst\n".to_string(), Ok(()))
    );
}

/// `#1` の箱を `unpack` の文で分解する。verifier はコンストラクタの定義を知らないので、タグとフィールドの数の違いは
/// 実行して初めて分かり、インタプリタの内部の誤りになる。分解したフィールドを捨てないので、verifier を通さない。
fn unpack_of_tag_one(unpack: &str) -> Result<(), RuntimeError> {
    let text = format!(
        "\
fn main() -> unit {{
  let s.0: obj = const \"field\"
  let p.1: obj = con #1(s.0)
  {unpack}
  return ()
}}
"
    );
    run_core_unverified(&text).1
}

#[test]
fn an_unpack_with_a_different_tag_is_an_internal_error() {
    assert_eq!(
        unpack_of_tag_one("unpack p.1 #0(a.2: obj)"),
        Err(RuntimeError::Fault {
            fault: Fault::Internal("an unpack names a different tag than the value has"),
            function: "main".to_string(),
            at: None,
        })
    );
}

#[test]
fn an_unpack_with_a_different_number_of_fields_is_an_internal_error() {
    assert_eq!(
        unpack_of_tag_one("unpack p.1 #1(a.2: obj, b.3: obj)"),
        Err(RuntimeError::Fault {
            fault: Fault::Internal(
                "an unpack binds a different number of fields than the value has"
            ),
            function: "main".to_string(),
            at: None,
        })
    );
}

#[test]
fn an_unpack_of_a_value_that_is_not_an_object_is_an_internal_error() {
    // verifier は R8 で `obj` でない値の `unpack` を拒むので、通さずに実行する
    let text = "\
fn main() -> unit {
  let n.0: int = extern Prelude.+(1, 2)
  unpack n.0 #0(a.1: obj)
  return ()
}
";
    assert_eq!(
        run_core_unverified(text).1,
        Err(RuntimeError::Fault {
            fault: Fault::Internal("an unpack of a value that is not an object"),
            function: "main".to_string(),
            at: None,
        })
    );
}

#[test]
fn an_unpack_of_an_object_that_is_not_data_is_an_internal_error() {
    // 文字列も `obj` なので verifier は通すが、コンストラクタの値ではない
    let text = "\
fn main() -> unit {
  let s.0: obj = extern Prelude.show_int(1)
  unpack s.0 #0(a.1: obj)
  return ()
}
";
    assert_eq!(
        run_core_unverified(text).1,
        Err(RuntimeError::Fault {
            fault: Fault::Internal("an unpack of an object that is not data"),
            function: "main".to_string(),
            at: None,
        })
    );
}

// release

/// `d.1` の箱を `release` で手放し、`s.0` がフィールドの参照を受け取る。`s.0` はフィールドと同じ値を指すので、名前に
/// 書ける。文字列のリテラルは不死なので、`show_int` で作った文字列をフィールドに入れ、解放の誤りが `debug_heap`
/// に見えるようにする。`shared` が真なら `release` の前に箱を複製し、`release` は共有の側を通る。
fn release_one_field(shared: bool) -> String {
    let (dup, decref) = if shared {
        ("  dup d.1\n", "  decref d.1\n")
    } else {
        ("", "")
    };
    format!(
        "\
fn main() -> unit {{
  let s.0: obj = extern Prelude.show_int(7)
  let d.1: tobj = con #1(s.0)
{dup}  release d.1 #1(s.0)
  let o.2: unit = extern Prelude.println(s.0)
{decref}  return o.2
}}
"
    )
}

#[test]
fn a_release_of_a_unique_box_hands_its_field_over() {
    assert_eq!(
        run_core(&release_one_field(false)),
        ("7\n".to_string(), Ok(()))
    );
}

#[test]
fn a_release_of_a_shared_box_dups_its_field() {
    assert_eq!(
        run_core(&release_one_field(true)),
        ("7\n".to_string(), Ok(()))
    );
}

#[test]
fn a_release_keeps_one_value_in_two_fields_twice() {
    let text = "\
fn main() -> unit {
  let s.0: obj = extern Prelude.show_int(7)
  dup s.0
  let d.1: obj = con #0(s.0, s.0)
  release d.1 #0(s.0, s.0)
  let t.2: obj = extern Prelude.++(s.0, s.0)
  let o.3: unit = extern Prelude.println(t.2)
  return o.3
}
";
    assert_eq!(run_core(text), ("77\n".to_string(), Ok(())));
}

/// `#1` の箱を `release` で手放す。タグとフィールドの数の違いは、`unpack` と同じく実行して初めて分かる。どの形も
/// verifier が拒むので、通さずに実行する。
fn release_of_tag_one(release: &str) -> Result<(), RuntimeError> {
    let text = format!(
        "\
fn main() -> unit {{
  let s.0: obj = extern Prelude.show_int(7)
  let p.1: obj = con #1(s.0)
  {release}
  return ()
}}
"
    );
    run_core_unverified(&text).1
}

#[test]
fn a_release_of_another_layout_is_an_internal_error() {
    let fault = Err(RuntimeError::Fault {
        fault: Fault::Internal(
            "a release names a tag and number of fields the value does not have",
        ),
        function: "main".to_string(),
        at: None,
    });
    assert_eq!(release_of_tag_one("release p.1 #0(s.0)"), fault);
    assert_eq!(release_of_tag_one("release p.1 #1(s.0, _)"), fault);
    // 文字列も `obj` だが、コンストラクタの値ではない
    assert_eq!(release_of_tag_one("release s.0 #1(s.0)"), fault);
}

#[test]
fn a_release_of_a_value_that_is_not_an_object_is_an_internal_error() {
    let text = "\
fn main() -> unit {
  let n.0: int = extern Prelude.+(1, 2)
  release n.0 #0(n.0)
  return ()
}
";
    assert_eq!(
        run_core_unverified(text).1,
        Err(RuntimeError::Fault {
            fault: Fault::Internal("a release of a value that is not an object"),
            function: "main".to_string(),
            at: None,
        })
    );
}
