//! REPL の入力の変換 (docs/spec/object-model.md の「セッションの根」、「名前」、「実行と表示」)。

use crate::common::{function, session_core_text, session_lir_text};
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
fn a_binding_is_read_from_its_root_and_the_result_is_returned_as_text() {
    // 根は `tobj` なので、型の Repr への変換は box の挿入が入れる。`Show` を書いた型の値は Prelude の `show` で文字列に
    // し、書くのは `Repl` である
    insta::assert_snapshot!(session_core_text(&["let x = 41", "x + 1"], Pass::Translate), @r#"
    fn input$2(p.0: unit) -> int {
      let r.1: int = prim root_get[1]()
      let t.2: int = extern Prelude.+(r.1, 1)
      return t.2
    }
    fn entry$input$2() -> obj {
      let t.0: int = call input$2(())
      let s.1: obj = extern "Prelude.Show Int.show"(t.0)
      return s.1
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
fn a_function_input_is_called_and_returns_fn() {
    let shown = session_core_text(&["fn x -> x + 1"], Pass::Translate);
    insta::assert_snapshot!(function(&shown, "entry$input$1"), @r#"
    fn entry$input$1() -> obj {
      let t.0: tobj = call input$1(())
      let s.1: obj = const "<fn>"
      return s.1
    }
    "#);
}

#[test]
fn a_structural_input_calls_its_display_function() {
    let shown = session_core_text(
        &["data B = | B (Int -> Int) Int", "B (fn x -> x) (-3)"],
        Pass::Translate,
    );
    insta::assert_snapshot!(function(&shown, "entry$input$2"), @r#"
    fn entry$input$2() -> obj {
      let t.0: obj = call input$2(())
      let s.1: obj = const ""
      let r.2: obj = call display$2$0(s.1, 300, 0, 0, t.0)
      unpack r.2 (,) #0(s.3: obj, n.4: int)
      return s.3
    }
    "#);
    insta::assert_snapshot!(function(&shown, "display$2$0"), @r#"
    fn display$2$0(acc.0: obj, budget.1: int, depth.2: int, prec.3: int, x.4: obj) -> obj {
      let c.5: enum = extern "Prelude.Eq Int.=="(budget.1, 0)
      switch c.5 Prelude.Bool { #0 -> b1, #1 -> b2 }
    b1:
      let n.9: int = extern Prelude.-(budget.1, 1)
      let c.10: enum = extern "Prelude.Ord Int.>="(depth.2, 100)
      switch c.10 Prelude.Bool { #0 -> b3, #1 -> b4 }
    b2:
      let s.6: obj = const "..."
      let s.7: obj = extern Prelude.++(acc.0, s.6)
      let r.8: obj = con (,) #0(s.7, budget.1)
      return r.8
    b3:
      let n.14: int = extern Prelude.+(depth.2, 1)
      unpack x.4 B #0(a.15: tobj, a.16: int)
      let c.17: enum = extern "Prelude.Ord Int.>"(prec.3, 10)
      switch c.17 Prelude.Bool { #0 -> b5, #1 -> b6 }
    b4:
      let s.11: obj = const "..."
      let s.12: obj = extern Prelude.++(acc.0, s.11)
      let r.13: obj = con (,) #0(s.12, n.9)
      return r.13
    b5:
      jump b7(acc.0)
    b6:
      let s.18: obj = const "("
      let s.19: obj = extern Prelude.++(acc.0, s.18)
      jump b7(s.19)
    b7(acc.20: obj):
      let s.21: obj = const "B"
      let s.22: obj = extern Prelude.++(acc.20, s.21)
      let s.23: obj = const " "
      let s.24: obj = extern Prelude.++(s.22, s.23)
      let c.25: enum = extern "Prelude.Eq Int.=="(n.9, 0)
      switch c.25 Prelude.Bool { #0 -> b8, #1 -> b9 }
    b8:
      let n.28: int = extern Prelude.-(n.9, 1)
      let c.29: enum = extern "Prelude.Ord Int.>="(n.14, 100)
      switch c.29 Prelude.Bool { #0 -> b10, #1 -> b11 }
    b9:
      let s.26: obj = const "..."
      let s.27: obj = extern Prelude.++(s.24, s.26)
      jump b12(s.27, n.9)
    b10:
      let s.32: obj = const "<fn>"
      let s.33: obj = extern Prelude.++(s.24, s.32)
      jump b12(s.33, n.28)
    b11:
      let s.30: obj = const "..."
      let s.31: obj = extern Prelude.++(s.24, s.30)
      jump b12(s.31, n.28)
    b12(acc.34: obj, budget.35: int):
      let s.36: obj = const " "
      let s.37: obj = extern Prelude.++(acc.34, s.36)
      let c.38: enum = extern "Prelude.Eq Int.=="(budget.35, 0)
      switch c.38 Prelude.Bool { #0 -> b13, #1 -> b14 }
    b13:
      let n.41: int = extern Prelude.-(budget.35, 1)
      let c.42: enum = extern "Prelude.Ord Int.>="(n.14, 100)
      switch c.42 Prelude.Bool { #0 -> b15, #1 -> b16 }
    b14:
      let s.39: obj = const "..."
      let s.40: obj = extern Prelude.++(s.37, s.39)
      jump b17(s.40, budget.35)
    b15:
      let t.45: obj = call "Prelude.Show Int.show_prec"(11, a.16)
      let s.46: obj = extern Prelude.++(s.37, t.45)
      jump b17(s.46, n.41)
    b16:
      let s.43: obj = const "..."
      let s.44: obj = extern Prelude.++(s.37, s.43)
      jump b17(s.44, n.41)
    b17(acc.47: obj, budget.48: int):
      switch c.17 Prelude.Bool { #0 -> b18, #1 -> b19 }
    b18:
      jump b20(acc.47)
    b19:
      let s.49: obj = const ")"
      let s.50: obj = extern Prelude.++(acc.47, s.49)
      jump b20(s.50)
    b20(acc.51: obj):
      let r.52: obj = con (,) #0(acc.51, budget.48)
      return r.52
    }
    "#);
}

/// リストの背骨は補助の関数 `display$N$k$rest` がたどる。自分を末尾で呼ぶので、縮約が末尾呼び出しにし、長いリストでも
/// スタックが伸びない (docs/spec/core-ir.md の「表示関数」)。
#[test]
fn a_list_display_walks_its_spine_in_a_tail_calling_helper() {
    let lowered = session_lir_text(&["[fn x -> x + 1]"]);
    let main = function(&lowered, "display$1$0");
    assert!(main.contains("tail call display$1$0$rest("), "{main}");
    let rest = function(&lowered, "display$1$0$rest");
    assert!(rest.contains("tail call display$1$0$rest("), "{rest}");
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
    fn entry$input$2() -> obj {
      let t.0: int = call input$2(())
      let s.1: obj = extern "Prelude.Show Int.show"(t.0)
      return s.1
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
            "show (a, B)",
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

/// 表示関数の名前の一覧。`display$N$k$rest` は数えない。
fn displays(shown: &str) -> Vec<&str> {
    shown
        .lines()
        .filter_map(|line| line.strip_prefix("fn "))
        .map(|rest| &rest[..rest.find('(').expect("a function header")])
        .filter(|name| name.starts_with("display$") && !name.ends_with("$rest"))
        .collect()
}

/// 表示関数の名前は入力の番号と見つけた順の番号から作る。型の表示から作らないので、型が長くても名前は短い
/// (docs/spec/core-ir.md の「表示関数」)。
#[test]
fn display_functions_are_named_after_their_input() {
    let shown = session_core_text(
        &["let n = 1", "(n, fn x -> x + 1, [(n, n)])"],
        Pass::Translate,
    );
    assert_eq!(
        displays(&shown),
        ["display$2$0", "display$2$1", "display$2$2"]
    );
}

/// 入れ子の型は段ごとに新しい型をたどる。表示関数は段の上限 (100) と数の上限 (1000) で止まる。
#[test]
fn nested_types_are_cut_by_depth_and_by_count() {
    let shown = session_core_text(
        &[
            "data Nest a = | NNil | NCons a (Nest (a, a))",
            "NCons (fn x -> x + 1) NNil",
        ],
        Pass::Translate,
    );
    // 段 0 から 99 の `Nest` と、段 2 から 99 のタプル
    assert_eq!(displays(&shown).len(), 198);
    let shown = session_core_text(
        &[
            "data W a = | WNil | W a (W (a, Int)) (W (a, Bool))",
            "W (fn x -> x + 1) WNil WNil",
        ],
        Pass::Translate,
    );
    let names = displays(&shown);
    assert_eq!(names.len(), 1000);
    assert_eq!(names.last(), Some(&"display$2$999"));
}

/// 文脈が型引数で解けない instance は使わず、構造で書く。instance のメソッドは作らない
/// (docs/spec/core-ir.md の「表示関数」)。
#[test]
fn an_instance_whose_context_fails_is_not_used() {
    let shown = session_core_text(
        &[
            "data Box a = | Box a",
            "instance Show a => Show (Box a) where\n  show (Box x) = show x",
            "Box (fn x -> x + 1)",
        ],
        Pass::Translate,
    );
    assert_eq!(displays(&shown), ["display$3$0"]);
    assert!(!shown.contains("Show Box.show"), "{shown}");
}
