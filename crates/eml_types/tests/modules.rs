//! モジュールをまたぐ型検査 (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 5.2)。Prelude の末尾に本体のある
//! 関数を足したプログラムで、診断が位置のあるファイルを指すことを確かめる。

use std::fmt::Write;

use eml_diagnostics::{Label, SourceFiles};

/// 線形性のテストで使うエフェクト。Prelude に足す。
const CHOICE: &str = "pub effect Choice where\n  multi choose : Unit -> Bool\n";

/// Prelude の末尾に `extra` を足したプログラムを検査し、診断を「番号 文言」と、ラベルごとの「ファイル "指す文字列" 文言」
/// の行にする。Prelude の行番号は Prelude を変えるたびに動くので、位置は指す文字列で示す。
fn check_with_prelude(extra: &str, text: &str) -> String {
    let mut files = SourceFiles::new();
    let prelude = files.add(
        eml_hir::PRELUDE_PATH,
        format!("{}\n{extra}", eml_hir::PRELUDE_SOURCE),
    );
    let entry = files.add("test.em", text);
    let mut diagnostics = Vec::new();
    let trees = [prelude, entry].map(|file| {
        let (parse, errors) = eml_syntax::parse(file, files.text(file));
        assert!(errors.is_empty(), "{errors:?}");
        let (tree, stage) = eml_hir::item_tree(file, &parse.tree());
        diagnostics.extend(stage);
        tree
    });
    let (def_map, stage) = eml_hir::def_map(&trees);
    diagnostics.extend(stage);
    let (program, stage) = eml_hir::lower(&def_map, &trees);
    diagnostics.extend(stage);
    let (_, stage) = eml_types::check(&program);
    diagnostics.extend(stage);
    eml_diagnostics::sort_diagnostics(&mut diagnostics);
    let mut out = String::new();
    for d in &diagnostics {
        writeln!(out, "{} {}", d.code, d.message).unwrap();
        for label in std::iter::once(&d.primary).chain(&d.secondary) {
            writeln!(out, "  {}", shown(&files, label)).unwrap();
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
    let extra = "pub twice : File -> <IO> Unit\ntwice f =\n  close f\n  close f\n\npub dropped : File -> <IO> Unit\ndropped f = ()\n\npub discarded : File -> <IO> Unit\ndiscarded _ = ()\n\npub captured : File -> <IO> (Unit -> <IO> Unit)\ncaptured f = fn () -> close f\n";
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
