//! Core IR のテキストで、`data` の値の確保、`switch` と `unpack` による分解、`release`、`box` と `unbox` を確かめる
//! (docs/spec/core-ir.md)。共有された値の分解や、`tobj` の変数に入った引数のないコンストラクタは、ソースの `match`
//! からは狙って作りにくいので、ここで書く。

use eml_interp::{Fault, RuntimeError};

use crate::common::{run_core, run_core_unverified};

#[test]
fn a_tobj_variable_holding_a_tag_takes_its_case() {
    // 引数のないコンストラクタの値は、`tobj` の変数にも即値で入る
    let text = "\
layout Option { None, Some(tobj) }
fn main() -> unit {
  tail call show(#0)
}
fn show(d.0: tobj) -> unit {
  switch d.0 Option { #0 -> b1, #1(x.1: obj) -> b2 }
b1:
  decref d.0
  let s.2: obj = const \"none\"
  let o.3: unit = extern Prelude.println(s.2)
  return o.3
b2:
  decref d.0
  return ()
}
";
    assert_eq!(run_core(text), ("none\n".to_string(), Ok(())));
}

#[test]
fn a_case_with_a_different_number_of_fields_is_an_internal_error() {
    let text = "\
layout List { Nil, Cons(tobj, tobj) }
layout Option { None, Some(tobj) }
fn main() -> unit {
  let s.0: obj = const \"field\"
  let d.1: tobj = con Option #1(s.0)
  switch d.1 List { #0 -> b1, #1(x.2: obj, y.3: obj) -> b2 }
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

#[test]
fn a_value_read_with_another_layout_passes_the_verifier_and_faults() {
    // verifier は命令をその命令が指す配置と比べるだけで、値を作った配置を追わない (docs/spec/core-ir.md の
    // 「データの配置」)。`Option` で作った値を `List` として分解する IR は verifier を通り、機械の見張りで止まる
    let text = "\
layout List { Nil, Cons(tobj, tobj) }
layout Option { None, Some(tobj) }
fn main() -> unit {
  let s.0: obj = const \"field\"
  let d.1: tobj = con Option #1(s.0)
  switch d.1 List { #0 -> b1, #1(x.2: obj, y.3: obj) -> b2 }
b1:
  decref d.1
  return ()
b2:
  release d.1 List #1(x.2, y.3)
  decref x.2
  decref y.3
  return ()
}
";
    assert_eq!(
        run_core(text).1,
        Err(RuntimeError::Fault {
            fault: Fault::Internal(
                "a switch case binds a different number of fields than the value has"
            ),
            function: "main".to_string(),
            at: None,
        })
    );
}

/// 文字列のリテラルの case に一致する値と、`default` に進む値。`Switch` は文字列を読むだけで、どの行き先も文字列を
/// 1回だけ手放す。
const STRING_SWITCH: &str = "\
fn main() -> unit {
  let s.0: obj = const \"a\"
  let n.1: int = call pick(s.0)
  let s.2: obj = const \"b\"
  let n.3: int = call pick(s.2) save [n.1]
  let t.4: int = extern Prelude.+(n.1, n.3)
  let t.5: obj = extern \"Prelude.Show Int.show\"(t.4)
  let o.6: unit = extern Prelude.println(t.5)
  return o.6
}
fn pick(s.0: obj) -> int {
  switch s.0 { \"a\" -> b1, _ -> b2 }
b1:
  decref s.0
  return 1
b2:
  decref s.0
  return 2
}
";

#[test]
fn a_string_switch_leaves_the_string_to_its_targets() {
    assert_eq!(run_core(STRING_SWITCH), ("3\n".to_string(), Ok(())));
}

#[test]
fn a_value_with_fields_that_goes_to_the_default_is_released() {
    // `default` はフィールドを束縛しないので、行き先は値を `decref` で手放す
    let text = "\
layout Option { None, Some(tobj) }
fn main() -> unit {
  let s.0: obj = const \"field\"
  let d.1: tobj = con Option #1(s.0)
  switch d.1 Option { #0 -> b1, _ -> b2 }
b1:
  decref d.1
  return ()
b2:
  decref d.1
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

/// `Pair` の箱を2回分解する。`unpack` は箱を読むだけなので、1回目の後も箱は残る。1回目はフィールドを複製し、2回目は
/// `release` で箱を手放してフィールドを受け取る。
const UNPACK_TWICE: &str = "\
layout Pair { Pair(tobj, int) }
fn main() -> unit {
  let s.0: obj = const \"first\"
  let p.1: obj = con Pair #0(s.0, 2)
  unpack p.1 Pair #0(a.2: obj, n.3: int)
  dup a.2
  let o.4: unit = extern Prelude.println(a.2)
  unpack p.1 Pair #0(b.5: obj, m.6: int)
  release p.1 Pair #0(b.5, _)
  let o.7: unit = extern Prelude.println(b.5)
  return o.7
}
";

#[test]
fn an_unpack_reads_the_fields_without_taking_the_box() {
    assert_eq!(
        run_core(UNPACK_TWICE),
        ("first\nfirst\n".to_string(), Ok(()))
    );
}

/// `#1` の箱を `unpack` の文で分解する。タグとフィールドの数の違いは実行して初めて分かり、インタプリタの内部の
/// 誤りになる。verifier は値を作った配置を追わないので、この誤りは verifier を通った IR でも起きうる
/// (`a_value_read_with_another_layout_passes_the_verifier_and_faults`)。ここの IR は `tobj` の配置の値を `obj` に
/// 束縛して `unpack` するので verifier が拒み、通さずに実行する。
fn unpack_of_tag_one(unpack: &str) -> Result<(), RuntimeError> {
    let text = format!(
        "\
layout Option {{ None, Some(tobj) }}
fn main() -> unit {{
  let s.0: obj = const \"field\"
  let p.1: obj = con Option #1(s.0)
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
        unpack_of_tag_one("unpack p.1 Option #0(a.2: obj)"),
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
        unpack_of_tag_one("unpack p.1 Option #1(a.2: obj, b.3: obj)"),
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
layout Box { Box(tobj) }
fn main() -> unit {
  let n.0: int = extern Prelude.+(1, 2)
  unpack n.0 Box #0(a.1: obj)
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
layout Box { Box(tobj) }
fn main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(1)
  unpack s.0 Box #0(a.1: obj)
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

/// `switch` で `d.1` を分解し、行き先が `release` で箱を手放して、フィールドの `x.2` が参照を受け取る。文字列の
/// リテラルは不死なので、`show` で作った文字列をフィールドに入れ、解放の誤りが `debug_heap` に見えるようにする。
/// `shared` が真なら `switch` の前に箱を複製し、`release` は共有の側を通る。
fn release_one_field(shared: bool) -> String {
    let (dup, decref) = if shared {
        ("  dup d.1\n", "  decref d.1\n")
    } else {
        ("", "")
    };
    format!(
        "\
layout Option {{ None, Some(tobj) }}
fn main() -> unit {{
  let s.0: obj = extern \"Prelude.Show Int.show\"(7)
  let d.1: tobj = con Option #1(s.0)
{dup}  switch d.1 Option {{ #0 -> b1, #1(x.2: obj) -> b2 }}
b1:
{decref}  decref d.1
  return ()
b2:
  release d.1 Option #1(x.2)
  let o.3: unit = extern Prelude.println(x.2)
{decref}  return o.3
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
layout (,) { (,)(tobj, tobj) }
fn main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(7)
  dup s.0
  let d.1: obj = con (,) #0(s.0, s.0)
  unpack d.1 (,) #0(x.2: obj, y.3: obj)
  release d.1 (,) #0(x.2, y.3)
  let t.4: obj = extern Prelude.++(x.2, y.3)
  let o.5: unit = extern Prelude.println(t.4)
  return o.5
}
";
    assert_eq!(run_core(text), ("77\n".to_string(), Ok(())));
}

/// `#1` の箱を `release` で手放す。タグとフィールドの数の違いは、`unpack` と同じく実行して初めて分かり、verifier
/// を通った IR でも起きうる。ここの形はどれも verifier が拒むので、通さずに実行する。
fn release_of_tag_one(release: &str) -> Result<(), RuntimeError> {
    let text = format!(
        "\
layout Option {{ None, Some(tobj) }}
fn main() -> unit {{
  let s.0: obj = extern \"Prelude.Show Int.show\"(7)
  let p.1: obj = con Option #1(s.0)
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
    assert_eq!(release_of_tag_one("release p.1 Option #0(s.0)"), fault);
    assert_eq!(release_of_tag_one("release p.1 Option #1(s.0, _)"), fault);
    // 文字列も `obj` だが、コンストラクタの値ではない
    assert_eq!(release_of_tag_one("release s.0 Option #1(s.0)"), fault);
}

#[test]
fn a_release_of_a_value_that_is_not_an_object_is_an_internal_error() {
    let text = "\
layout Box { Box(tobj) }
fn main() -> unit {
  let n.0: int = extern Prelude.+(1, 2)
  release n.0 Box #0(n.0)
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

// box と unbox

#[test]
fn a_box_and_an_unbox_pass_the_value_through() {
    let text = "\
layout Prelude.Bool { False, True }
fn main() -> unit {
  let b.0: tobj = box 41
  let n.1: int = unbox b.0
  decref b.0
  let c.2: enum = extern \"Prelude.Ord Int.<\"(n.1, 50)
  let e.3: tobj = box c.2
  let d.4: enum = unbox e.3
  decref e.3
  switch d.4 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  return ()
b2:
  let s.5: obj = extern \"Prelude.Show Int.show\"(n.1)
  let o.6: unit = extern Prelude.println(s.5)
  return o.6
}
";
    assert_eq!(run_core(text), ("41\n".to_string(), Ok(())));
}

#[test]
fn an_unbox_of_a_heap_object_passes_the_verifier_and_faults() {
    // verifier は `tobj` の値がスカラーを入れたものかヒープの物体かを追わない (docs/spec/core-ir.md の「データの
    // 配置」)。`tobj` のフィールドに入れた文字列を `unbox` する IR は verifier を通り、機械の見張りで止まる
    let text = "\
layout Box { Box(tobj) }
fn main() -> unit {
  let s.0: obj = const \"a\"
  let d.1: obj = con Box #0(s.0)
  unpack d.1 Box #0(x.2: tobj)
  let n.3: int = unbox x.2
  decref d.1
  return ()
}
";
    assert_eq!(
        run_core(text).1,
        Err(RuntimeError::Fault {
            fault: Fault::Internal("an unbox of a heap object"),
            function: "main".to_string(),
            at: None,
        })
    );
}

#[test]
fn a_box_of_a_heap_object_is_an_internal_error() {
    // verifier は `obj` の変数の `box` を拒むので、通さずに実行する
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"a\"
  let b.1: tobj = box s.0
  return ()
}
";
    assert_eq!(
        run_core_unverified(text).1,
        Err(RuntimeError::Fault {
            fault: Fault::Internal("a box of a heap object"),
            function: "main".to_string(),
            at: None,
        })
    );
}
