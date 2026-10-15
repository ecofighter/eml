//! LIR のテキストで、`data` の値の確保、`switch` と `unpack` による分解、`release`、`box` と `unbox` を確かめる
//! (docs/spec/lir.md)。共有された値の分解や、`tobj` の変数に入った引数のないコンストラクタは、ソースの `match`
//! からは狙って作りにくいので、ここで書く。

use eml_vm::{VmError, VmFault};

use crate::common::{run_lir, run_unverified};

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
    assert_eq!(run_lir(text), ("none\n".to_string(), Ok(())));
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
        run_unverified(text).1,
        Err(VmError::Fault {
            fault: VmFault::Internal(
                "a switch case binds a different number of fields than the value has"
            ),
            function: "main".to_string(),
            at: None,
        })
    );
}

#[test]
fn a_value_read_with_another_layout_passes_the_verifier_and_faults() {
    // verifier は命令をその命令が指す配置と比べるだけで、値を作った配置を追わない (docs/spec/lir.md の
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
        run_lir(text).1,
        Err(VmError::Fault {
            fault: VmFault::Internal(
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
    assert_eq!(run_lir(STRING_SWITCH), ("3\n".to_string(), Ok(())));
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
    assert_eq!(run_lir(text), ("default\n".to_string(), Ok(())));
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
    assert_eq!(run_lir(text), ("two\n".to_string(), Ok(())));
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
        run_lir(UNPACK_TWICE),
        ("first\nfirst\n".to_string(), Ok(()))
    );
}

/// `#1` の箱を `unpack` の文で分解する。タグとフィールドの数の違いは実行して初めて分かり、VM の内部の
/// 誤りになる。verifier は値を作った配置を追わないので、この誤りは verifier を通った IR でも起きうる
/// (`a_value_read_with_another_layout_passes_the_verifier_and_faults`)。ここの IR は `tobj` の配置の値を `obj` に
/// 束縛して `unpack` するので verifier が拒み、通さずに実行する。
fn unpack_of_tag_one(unpack: &str) -> Result<(), VmError> {
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
    run_unverified(&text).1
}

#[test]
fn an_unpack_with_a_different_tag_is_an_internal_error() {
    assert_eq!(
        unpack_of_tag_one("unpack p.1 Option #0(a.2: obj)"),
        Err(VmError::Fault {
            fault: VmFault::Internal(
                "an unpack names a tag and number of fields the value does not have"
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
        run_unverified(text).1,
        Err(VmError::Fault {
            fault: VmFault::Internal(
                "an unpack names a tag and number of fields the value does not have"
            ),
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
        run_unverified(text).1,
        Err(VmError::Fault {
            fault: VmFault::Internal(
                "an unpack names a tag and number of fields the value does not have"
            ),
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
        run_lir(&release_one_field(false)),
        ("7\n".to_string(), Ok(()))
    );
}

#[test]
fn a_release_of_a_shared_box_dups_its_field() {
    assert_eq!(
        run_lir(&release_one_field(true)),
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
    assert_eq!(run_lir(text), ("77\n".to_string(), Ok(())));
}

/// `#1` の箱を `release` で手放す。タグとフィールドの数の違いは、`unpack` と同じく実行して初めて分かり、verifier
/// を通った IR でも起きうる。ここの形はどれも verifier が拒むので、通さずに実行する。
fn release_of_tag_one(release: &str) -> Result<(), VmError> {
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
    run_unverified(&text).1
}

#[test]
fn a_release_of_another_layout_is_an_internal_error() {
    let fault = Err(VmError::Fault {
        fault: VmFault::Internal(
            "a release names a tag and number of fields the value does not have",
        ),
        function: "main".to_string(),
        at: None,
    });
    assert_eq!(release_of_tag_one("release p.1 Option #0(s.0)"), fault);
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
        run_unverified(text).1,
        Err(VmError::Fault {
            fault: VmFault::Internal(
                "a release names a tag and number of fields the value does not have"
            ),
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
    assert_eq!(run_lir(text), ("41\n".to_string(), Ok(())));
}

const BOOL: &str = "layout Bool { False, True }\n";

fn show_bool(program: &str) -> String {
    format!(
        "{BOOL}{program}\
fn say(b.0: enum) -> unit {{
  switch b.0 Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let s.1: obj = const \"no\"
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
b2:
  let s.3: obj = const \"yes\"
  let u.4: unit = extern Prelude.println(s.3)
  return u.4
}}
"
    )
}

/// 生の `enum` の 1 は即値の 0 と同じビット列なので、語の形で分けると `#0` へ進んでしまう
/// (docs/spec/vm.md の「switch」)。
#[test]
fn a_raw_true_takes_the_true_case() {
    let text = show_bool(
        "fn main() -> unit {\n  let b.0: enum = extern \"Prelude.Ord Int.<\"(1, 2)\n  tail call say(b.0)\n}\n",
    );
    assert_eq!(run_lir(&text), ("yes\n".to_string(), Ok(())));
}

/// 生の `enum` の 2 は偶数でポインタの形をしているので、語の形で分けると `meta` を読みに行ってしまう。
#[test]
fn an_enum_tag_two_is_not_read_as_a_pointer() {
    let text = "\
layout Color { Red, Green, Blue }
fn main() -> unit {
  tail call name(#2)
}
fn name(c.0: enum) -> unit {
  switch c.0 Color { #0 -> b1, #1 -> b2, #2 -> b3 }
b1:
  return ()
b2:
  return ()
b3:
  let s.1: obj = const \"blue\"
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
";
    assert_eq!(run_lir(text), ("blue\n".to_string(), Ok(())));
}

/// 定数と非 RC の値の `drop` には、何の命令も出さない。生のビット列はポインタに見えることがあるためである。
#[test]
fn a_drop_of_a_raw_value_does_nothing() {
    let text = "\
fn main() -> unit {
  let n.0: int = extern \"Prelude.+\"(1, 1)
  let u.1: unit = drop n.0
  let v.2: unit = drop #2
  return v.2
}
";
    assert_eq!(run_lir(text), (String::new(), Ok(())));
}

#[test]
fn a_big_int_round_trips_through_a_box() {
    let text = "\
fn main() -> unit {
  let b.0: tobj = box 9223372036854775807
  let n.1: int = unbox b.0
  decref b.0
  let m.2: int = extern \"Prelude.-\"(n.1, 1)
  let c.3: tobj = box m.2
  let k.4: int = unbox c.3
  decref c.3
  let s.5: obj = extern \"Prelude.Show Int.show\"(k.4)
  let u.6: unit = extern Prelude.println(s.5)
  return u.6
}
";
    assert_eq!(run_lir(text), ("9223372036854775806\n".to_string(), Ok(())));
}

/// 印が `lin` の `con` は `lin` ビットを立てるので、検査モードはその `dup` を断る。verifier の V1 も断るので、通さずに
/// 実行する。
#[test]
fn a_lin_con_sets_the_lin_bit() {
    let text = "\
layout Box { Box(obj) }
fn main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(1)
  let b.1: obj lin = con Box #0(s.0)
  dup b.1
  return ()
}
";
    let (_, result) = run_unverified(text);
    assert_eq!(
        result.unwrap_err().to_string(),
        "a Lin object is shared in `main`"
    );
}

/// `obj` のデータを `tobj` の変数へ入れて `unbox` する IR は verifier が拒むので、通さずに実行する。データの物体を
/// `unbox` すると `unbox_int` の `WrongKind` になる。
#[test]
fn an_unbox_of_a_data_object_is_a_heap_error() {
    let text = "\
layout Box { Box(obj) }
fn main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(1)
  let b.1: tobj = con Box #0(s.0)
  let n.2: int = unbox b.1
  decref b.1
  return ()
}
";
    let (_, result) = run_unverified(text);
    assert!(matches!(
        result,
        Err(VmError::Fault {
            fault: VmFault::Extern(eml_rt::Fault::Heap(eml_rt::HeapError::WrongKind)),
            ..
        })
    ));
}

/// 走査する語が 255 個以上のデータでは、ペイロードの先頭に数の語がある。最後のフィールドを読めることを確かめる。
#[test]
fn a_constructor_with_many_scanned_fields_reads_its_last_field() {
    let n = 300;
    let fields = vec!["obj"; n].join(", ");
    let mut text = format!("layout Wide {{ Wide({fields}) }}\nfn main() -> unit {{\n");
    for i in 0..n {
        text += &format!("  let s.{i}: obj = extern \"Prelude.Show Int.show\"({i})\n");
    }
    let args: Vec<String> = (0..n).map(|i| format!("s.{i}")).collect();
    text += &format!("  let w.{n}: obj = con Wide #0({})\n", args.join(", "));
    let binds: Vec<String> = (0..n).map(|i| format!("f.{}: obj", n + 1 + i)).collect();
    text += &format!("  unpack w.{n} Wide #0({})\n", binds.join(", "));
    let last = 2 * n;
    text += &format!("  dup f.{last}\n");
    text += &format!("  decref w.{n}\n");
    text += &format!(
        "  let u.{}: unit = extern Prelude.println(f.{last})\n",
        2 * n + 1
    );
    text += &format!("  return u.{}\n}}\n", 2 * n + 1);
    assert_eq!(run_lir(&text), (format!("{}\n", n - 1), Ok(())));
}

/// scrutinee が変数なので、行き先は実行時に決まる。負の鍵と 32 ビットを超える鍵は、オペランドの下位と上位の 2 語に
/// 分けて置くので、それぞれの符号と桁を保って比べられることを確かめる。
#[test]
fn an_int_variable_switch_matches_negative_and_wide_keys() {
    fn pick(n: i64) -> String {
        let text = format!(
            "\
fn main() -> unit {{
  let n.0: int = extern \"Prelude.+\"({n}, 0)
  switch n.0 {{ -3 -> b1, 5000000000 -> b2, _ -> b3 }}
b1:
  let s.1: obj = const \"negative\"
  let o.2: unit = extern Prelude.println(s.1)
  return o.2
b2:
  let s.3: obj = const \"wide\"
  let o.4: unit = extern Prelude.println(s.3)
  return o.4
b3:
  let s.5: obj = const \"default\"
  let o.6: unit = extern Prelude.println(s.5)
  return o.6
}}
"
        );
        run_lir(&text).0
    }
    assert_eq!(pick(-3), "negative\n");
    assert_eq!(pick(5_000_000_000), "wide\n");
    assert_eq!(pick(3), "default\n");
    assert_eq!(pick(5), "default\n");
}

/// `obj` の scrutinee は、レイアウトの全部のコンストラクタがフィールドを持つ。先頭でないケースを選び、フィールドを読む。
#[test]
fn an_obj_switch_takes_a_non_first_case_and_reads_its_field() {
    let text = "\
layout Shape { Circle(tobj), Square(tobj) }
fn main() -> unit {
  let s.0: obj = const \"square\"
  let d.1: obj = con Shape #1(s.0)
  tail call name(d.1)
}
fn name(d.0: obj) -> unit {
  switch d.0 Shape { #0(a.1: obj) -> b1, #1(b.2: obj) -> b2 }
b1:
  release d.0 Shape #0(a.1)
  let o.3: unit = extern Prelude.println(a.1)
  return o.3
b2:
  release d.0 Shape #1(b.2)
  let o.4: unit = extern Prelude.println(b.2)
  return o.4
}
";
    assert_eq!(run_lir(text), ("square\n".to_string(), Ok(())));
}

/// `codegen.rs` の同名のテストと同じ LIR を実行する。死んだ scrutinee のスロットを case のフィールドが共有すると、
/// 2つ目のフィールドを読み違える。
#[test]
fn a_dead_switch_scrutinee_leaves_the_case_fields_readable() {
    let text = "\
layout Inner { Inner(obj, obj) }
layout Outer { Outer(obj) }
fn main() -> unit {
  let a.0: obj = const \"a\"
  let b.1: obj = const \"b\"
  let i.2: obj = con Inner #0(a.0, b.1)
  let o.3: obj = con Outer #0(i.2)
  tail call go(o.3)
}
fn go(o.0: obj) -> unit {
  unpack o.0 Outer #0(f.1: obj)
  switch f.1 Inner { #0(a.2: obj, b.3: obj) -> b1 }
b1:
  dup a.2
  dup b.3
  decref o.0
  let u.4: unit = extern Prelude.println(a.2)
  let v.5: unit = extern Prelude.println(b.3)
  return v.5
}
";
    assert_eq!(run_lir(text), ("a\nb\n".to_string(), Ok(())));
}
