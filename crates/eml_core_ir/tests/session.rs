//! REPL の入力の変換 (docs/superpowers/specs/2026-10-15-s11a-repl-session-design.md の「根」、「名前」、「実行と表示」)。

use crate::common::{
    function, session_core_text, session_core_text_with_positions, session_lir_text,
};
use eml_core_ir::Pass;

#[test]
fn a_let_input_puts_its_value_in_a_root() {
    insta::assert_snapshot!(session_core_text(&["let x = 41"], Pass::Translate), @r"
    fn input$1(p.0: unit) -> int {
      return 41
    }
    fn entry$input$1() -> unit {
      let t.0: int = call input$1(())
      let u.1: unit = prim root_set[1](t.0)
      return u.1
    }
    ");
}

#[test]
fn a_binding_is_read_from_its_root_and_the_result_is_shown() {
    // 根は `tobj` なので、型の Repr への変換は box の挿入が入れる。表示は Prelude の `show` と extern の `println` で行う
    insta::assert_snapshot!(session_core_text(&["let x = 41", "x + 1"], Pass::Translate), @r#"
    fn input$2(p.0: unit) -> int {
      let r.1: int = prim root_get[1]()
      let t.2: int = extern Prelude.+(r.1, 1)
      return t.2
    }
    fn entry$input$2() -> unit {
      let t.0: int = call input$2(())
      let s.1: obj = extern "Prelude.Show Int.show"(t.0)
      let u.2: unit = extern Prelude.println(s.1)
      return u.2
    }
    "#);
}

#[test]
fn roots_are_boxed_and_unboxed_after_the_lir_passes() {
    let stored = session_lir_text(&["let x = 41"]);
    insta::assert_snapshot!(function(&stored, "entry$input$1"), @r"
    fn entry$input$1() -> unit {
      let t.0: int = call input$1(())
      let t.2: tobj = box t.0
      let u.1: unit = prim root_set[1](t.2)
      return u.1
    }
    ");
    let read = session_lir_text(&["let x = 41", "x + 1"]);
    insta::assert_snapshot!(function(&read, "input$2"), @r"
    fn input$2(p.0: unit) -> int {
      let r.3: tobj = prim root_get[1]()
      let r.1: int = unbox r.3
      decref r.3
      let t.2: int = extern Prelude.+(r.1, 1)
      return t.2
    }
    ");
}

#[test]
fn a_function_definition_reads_a_binding_from_its_root() {
    let shown = session_core_text(
        &["let base = 10", "f : Int -> Int\nf n = n + base", "f 1"],
        Pass::Translate,
    );
    insta::assert_snapshot!(function(&shown, "f"), @r"
    fn f(n.0: int) -> int {
      let r.1: int = prim root_get[1]()
      let t.2: int = extern Prelude.+(n.0, r.1)
      return t.2
    }
    ");
}

#[test]
fn a_value_without_show_prints_its_type_at_the_input() {
    // 表示の失敗は入力の式の先頭の位置で報告する
    let shown = session_core_text_with_positions(&["fn x -> x + 1"]);
    insta::assert_snapshot!(function(&shown, "entry$input$1"), @r#"
    fn entry$input$1() -> unit {
      let t.0: tobj = call input$1(())
      let s.1: obj = const "_ : Int -> Int"
      let u.2: unit = extern Prelude.println(s.1) @"<input 1>":1:1
      return u.2
    }
    "#);
}

#[test]
fn a_unit_result_is_returned_without_printing() {
    let shown = session_core_text(&["println \"hi\""], Pass::Translate);
    insta::assert_snapshot!(function(&shown, "entry$input$1"), @r"
    fn entry$input$1() -> unit {
      let t.0: unit = call input$1(())
      return t.0
    }
    ");
}

#[test]
fn the_display_calls_the_prelude_show_even_when_shadowed() {
    let shown = session_core_text(
        &["show : Int -> String\nshow n = \"mine\"", "41"],
        Pass::Translate,
    );
    insta::assert_snapshot!(function(&shown, "entry$input$2"), @r#"
    fn entry$input$2() -> unit {
      let t.0: int = call input$2(())
      let s.1: obj = extern "Prelude.Show Int.show"(t.0)
      let u.2: unit = extern Prelude.println(s.1)
      return u.2
    }
    "#);
}

#[test]
fn a_redefined_function_is_named_after_its_input_and_old_callers_keep_the_old_one() {
    // 前の版の名前は後の入力で変わらない。`g` は入力 2 の時点の `f` を呼び続ける
    let shown = session_core_text(
        &[
            "f : Int -> Int\nf n = n",
            "g : Int -> Int\ng n = f n",
            "f : Int -> Int\nf n = n + 1",
            "g (f 1)",
        ],
        Pass::Translate,
    );
    insta::assert_snapshot!(function(&shown, "g"), @r"
    fn g(n.0: int) -> int {
      let t.1: int = call f(n.0)
      return t.1
    }
    ");
    insta::assert_snapshot!(function(&shown, "f#3"), @r#"
    fn "f#3"(n.0: int) -> int {
      let t.1: int = extern Prelude.+(n.0, 1)
      return t.1
    }
    "#);
}

#[test]
fn a_redefined_effect_gets_its_own_name_and_number() {
    // 名前で照らす追記がエフェクトを名前で引くので、定義し直したエフェクトは別の名前でなければならない
    let shown = session_core_text(
        &[
            "effect Ask where\n  ask : Unit -> Int",
            "effect Ask where\n  ask : Unit -> Int",
            "handle ask () with\n  | ask () k -> k 1\n  | return r -> r",
        ],
        Pass::Translate,
    );
    assert!(shown.contains("effect Ask { ask(unit) -> int }"), "{shown}");
    // エフェクトと配置の名前は `#` を含んでも1つの語として読み直せるので、関数の名前と違って引用符で囲まない
    assert!(
        shown.contains("effect Ask#2 { ask(unit) -> int }"),
        "{shown}"
    );
    assert!(shown.contains("handle Ask#2("), "{shown}");
    // Perceus の後の LIR も読み直せる (`session_lir_text` が確かめる)
    let lowered = session_lir_text(&[
        "effect Ask where\n  ask : Unit -> Int",
        "effect Ask where\n  ask : Unit -> Int",
        "handle ask () with\n  | ask () k -> k 1\n  | return r -> r",
    ]);
    assert!(lowered.contains("Ask#2"), "{lowered}");
}

#[test]
fn derived_functions_of_a_redefined_type_are_told_apart() {
    // 導出した関数の名前は、型の版を持つ。持たないと、2つの `C` の `show` が同じ名前になる
    let shown = session_core_text(
        &[
            "data C = | A deriving (Show)",
            "let a = A",
            "data C = | B deriving (Show)",
            "(a, B)",
        ],
        Pass::Translate,
    );
    assert!(shown.contains("fn \"Show C.show_prec\"("), "{shown}");
    assert!(shown.contains("fn \"Show C#3.show_prec\"("), "{shown}");
    assert!(shown.contains("layout C { A }"), "{shown}");
    assert!(shown.contains("layout C#3 { B }"), "{shown}");
}

#[test]
fn instance_names_carry_the_version_of_their_type_arguments() {
    // instance の名前の型は中段の名前で書くので、定義し直した型の instance は別の名前になる。Prelude と同じ名前の型は
    // `Main.` で修飾する
    let shown = session_core_text(
        &[
            "data C = | A",
            "id : a -> a\nid x = x",
            "let a = A",
            "data C = | B",
            "data Bool = | Yes",
            "(id a, id B, id Yes)",
        ],
        Pass::Translate,
    );
    assert!(shown.contains(r#"fn "id@[C]"("#), "{shown}");
    assert!(shown.contains(r#"fn "id@[C#4]"("#), "{shown}");
    assert!(shown.contains(r#"fn "id@[Main.Bool]"("#), "{shown}");
}

#[test]
fn instance_methods_of_a_redefined_type_keep_their_names() {
    // 利用者向けの表示名は古い型を `Color#1` と書くが、中段の名前は後の入力で変わらない。古い instance のメソッドは
    // `Show Color.show` のままで、新しい型の instance のメソッドは型の版を持つ
    let shown = session_core_text(
        &[
            "data Color = | Red",
            "instance Show Color where\n  show c = \"red\"",
            "let red = Red",
            "data Color = | Blue",
            "instance Show Color where\n  show c = \"blue\"",
            "(red, Blue)",
        ],
        Pass::Translate,
    );
    assert!(shown.contains(r#"fn "Show Color.show"("#), "{shown}");
    assert!(shown.contains(r#"fn "Show Color#4.show"("#), "{shown}");
}
