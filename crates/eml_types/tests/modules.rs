//! モジュールをまたぐ型検査 (docs/implementation/architecture.md の「`eml_types` の内部」)。Prelude の末尾に本体のある
//! 関数を足したプログラムで、診断が位置のあるファイルを指すことを確かめる。

use std::fmt::Write;

use eml_diagnostics::{Label, SourceFiles};

/// 線形性のテストで使うエフェクト。Prelude に足す。
const CHOICE: &str = "pub effect Choice where\n  multi choose : Unit -> Bool\n";

/// Prelude の末尾に `extra` を足したプログラムを検査し、診断を「番号 文言」と、ラベルごとの「ファイル "指す文字列" 文言」
/// の行にする。Prelude の行番号は Prelude を変えるたびに動くので、位置は指す文字列で示す。
fn check_with_prelude(extra: &str, text: &str) -> String {
    let lowered = eml_test_support::lower_with_prelude(
        &format!("{}\n{extra}", eml_hir::PRELUDE_SOURCE),
        text,
    );
    let (_, stage) = eml_types::check(&lowered.program);
    let mut diagnostics = lowered.diagnostics;
    diagnostics.extend(stage);
    eml_diagnostics::sort_diagnostics(&mut diagnostics);
    let files = &lowered.files;
    let mut out = String::new();
    for d in &diagnostics {
        writeln!(out, "{} {}", d.code, d.message).unwrap();
        for label in std::iter::once(&d.primary).chain(&d.secondary) {
            writeln!(out, "  {}", shown(files, label)).unwrap();
        }
    }
    out
}

fn shown(files: &SourceFiles, label: &Label) -> String {
    let text = &files.text(label.file)[label.range];
    format!("{} {text:?} {}", files.path(label.file), label.message)
}

#[test]
fn a_linear_misuse_in_the_prelude_points_into_the_prelude() {
    let extra = format!(
        "{CHOICE}\npub held : Unit -> <Choice, IO> Unit\nheld () =\n  let f = open \"a.txt\"\n  let b = choose ()\n  close f\n"
    );
    insta::assert_snapshot!(check_with_prelude(&extra, ""), @r#"
    E3006 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      Prelude.em "choose ()" this call may perform `choose`, a `multi` operation
      Prelude.em "f" `f` is bound here
      Prelude.em "choose" `choose` is declared `multi` here
    "#);
}

#[test]
fn a_carry_over_through_a_prelude_function_points_into_the_prelude() {
    let extra = format!(
        "{CHOICE}\npub keep : a -> (Unit -> <e> Unit) -> <e> a\nkeep x action =\n  action ()\n  x\n"
    );
    let text = "chooser : Unit -> <Choice> Unit\nchooser () =\n  let b = choose ()\n  ()\n\nkept : Unit -> <Choice, IO> Unit\nkept () =\n  let f = open \"a.txt\"\n  let g = keep f chooser\n  close g";
    insta::assert_snapshot!(check_with_prelude(&extra, text), @r#"
    E3006 `keep` keeps a linear value alive across a call that may resume more than once
      test.em "keep" `keep` is used here
      Prelude.em "action ()" `x` is kept alive across this call
    "#);
}

#[test]
fn linear_misuses_in_the_prelude_point_into_the_prelude() {
    let extra = "pub twice : File -> <IO> Unit\ntwice f =\n  close f\n  close f\n\npub dropped : File -> <IO> Unit\ndropped f = ()\n\npub discarded : File -> <IO> Unit\ndiscarded _ = ()\n";
    let shown = check_with_prelude(extra, "");
    // どの診断も、すべてのラベルが Prelude の中を指す
    for line in shown.lines().filter(|line| line.starts_with("  ")) {
        assert!(line.starts_with("  Prelude.em "), "{shown}");
    }
    for code in ["E3002", "E3003", "E3004"] {
        assert!(shown.contains(code), "{code}\n{shown}");
    }
}

#[test]
fn a_carry_over_through_a_composition_points_into_the_prelude() {
    // `>>` の本体は `g` を持ったまま `f` を呼ぶので、線形な `g` を `multi` の `f` と合成すると E3006 になる
    let text = "chooser : Unit -> <Choice> Unit\nchooser () =\n  let b = choose ()\n  ()\n\ncomposed : Unit -> <Choice, IO> Unit\ncomposed () =\n  let h = open \"a.txt\"\n  let k = chooser >> (fn u -> close h)\n  k ()";
    let shown = check_with_prelude(CHOICE, text);
    assert!(shown.starts_with("E3006 "), "{shown}");
    assert!(shown.contains("  test.em \">>\" "), "{shown}");
    assert!(shown.contains("  Prelude.em "), "{shown}");
}

/// 入口と依存先のモジュールを検査し、推論結果と診断を並べる。
fn check_modules(entry: &str, modules: &[(&str, &str)]) -> String {
    let checked = eml_test_support::check_files(entry, modules);
    eml_test_support::with_diagnostics(
        eml_types::dump(&checked.program, &checked.typed),
        &eml_test_support::full(&checked.files, &checked.diagnostics),
    )
}

#[test]
fn the_dump_shows_every_module_but_the_prelude() {
    let entry = "import Report\n\nmain : Unit -> <IO> Unit\nmain () = println (Report.render (Report.parse \"x\"))";
    let report = "pub data Row = | Row Int\n\npub parse : String -> Row\nparse s = Row 1\n\npub render : Row -> String\nrender r = match r with\n  | Row n -> show_int n";
    insta::assert_snapshot!(check_modules(entry, &[("Report.em", report)]), @r"
    -- Main
    main : Unit -> <IO> Unit
    -- Report
    parse : String -> Row
      s#0 : String
    render : Row -> String
      r#0 : Row
      n#1 : Int
    ");
}

#[test]
fn types_of_the_same_name_are_qualified_with_their_module() {
    let entry =
        "import Report\n\ndata Row = | Row Int\n\nconvert : Report.Row -> Row\nconvert r = r";
    insta::assert_snapshot!(check_modules(entry, &[("Report.em", "pub data Row = | Row Int")]), @r"
    -- Main
    convert : Report.Row -> Main.Row
      r#0 : Report.Row
    -- Report
    ---
    E2001 6:13 mismatched types
      6:13 expected `Main.Row`, found `Report.Row`
      5:11 expected because of the signature of `convert`
    ");
}

#[test]
fn a_user_unit_and_cont_are_told_apart_from_the_builtin_ones() {
    // 空のレコードは Prelude の `Unit` の表示名で書く。継続の型の `Cont` は修飾せず、ユーザーの `Cont` を修飾する
    let text = "data Unit = | U\n\ndata Cont = | C\n\neffect Ask where\n  ask : Int -> Int\n\nf : Cont -> Unit\nf c = ()\n\ng : Int -> Int\ng n =\n  handle ask n with\n    | ask x k -> resume k x";
    insta::assert_snapshot!(crate::common::check_text(text), @r"
    ask : Int -> <Ask> Int
    f : Main.Cont -> Main.Unit
      c#0 : Main.Cont
    g : Int -> Int
      n#0 : Int
      x#1 : Int
      k#2 : Cont Int Int <>
      $r#3 : Int
    ---
    E2001 9:7 mismatched types
      9:7 expected `Main.Unit`, found `Prelude.Unit`
      8:5 expected because of the signature of `f`
    ");
}
