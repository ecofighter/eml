//! モジュールをまたぐ型検査 (docs/implementation/architecture.md の「`eml_types` の内部」)。Prelude の末尾に本体のある
//! 関数を足したプログラムで、診断が位置のあるファイルを指すことを確かめる。

use std::fmt::Write;

use eml_diagnostics::{Label, SourceFiles};

/// 線形性のテストで使うエフェクト。Prelude に足す。
const CHOICE: &str = "pub effect Choice where\n  multi choose : Unit -> Bool\n";

/// Prelude の末尾に `extra` を足したプログラムを検査し、診断を「番号 文言」と、ラベルごとの「ファイル "指す文字列" 文言」
/// の行にする。標準ライブラリの行番号は本物のファイルを変えるたびに動くので、位置は指す文字列で示す。
fn check_with_prelude(extra: &str, text: &str) -> String {
    check_with_std_extras(extra, "", text)
}

/// `check_with_prelude` に加えて `Fs` の末尾にも `fs_extra` を足す。`File` は `Fs` の中にあるので、`File` を使う
/// 関数は Prelude に足せず、`Fs` に足す。
fn check_with_std_extras(prelude_extra: &str, fs_extra: &str, text: &str) -> String {
    let prelude = format!("{}\n{prelude_extra}", eml_hir::PRELUDE_SOURCE);
    let fs = format!("{}\n{fs_extra}", eml_hir::STD[1].1);
    let checked =
        eml_test_support::check_with_std(&[("Prelude.em", &prelude), ("Fs.em", &fs)], text);
    let files = checked.files();
    let mut out = String::new();
    for d in &checked.diagnostics {
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
fn a_linear_misuse_in_a_std_module_points_into_that_module() {
    let fs_extra = "pub held : Unit -> <Choice, IO> Unit\nheld () =\n  let f = open \"a.txt\"\n  let b = choose ()\n  close f\n";
    insta::assert_snapshot!(check_with_std_extras(CHOICE, fs_extra, ""), @r#"
    E3006 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      <std>/Fs.em "choose ()" this call may perform `choose`, a `multi` operation
      <std>/Fs.em "f" `f` is bound here
      <std>/Prelude.em "choose" `choose` is declared `multi` here
    "#);
}

#[test]
fn a_carry_over_through_a_prelude_function_points_into_the_prelude() {
    let extra = format!(
        "{CHOICE}\npub keep : a -> (Unit -> <e> Unit) -> <e> a\nkeep x action =\n  action ()\n  x\n"
    );
    let text = "chooser : Unit -> <Choice> Unit\nchooser () =\n  let b = choose ()\n  ()\n\nkept : Unit -> <Choice, IO> Unit\nkept () =\n  let f = Fs.open \"a.txt\"\n  let g = keep f chooser\n  Fs.close g";
    insta::assert_snapshot!(check_with_prelude(&extra, text), @r#"
    E3006 `keep` keeps a linear value alive across a call that may resume more than once
      test.em "keep" `keep` is used here
      <std>/Prelude.em "action ()" `x` is kept alive across this call
    "#);
}

#[test]
fn linear_misuses_in_a_std_module_point_into_that_module() {
    let fs_extra = "pub twice : File -> <IO> Unit\ntwice f =\n  close f\n  close f\n\npub dropped : File -> <IO> Unit\ndropped f = ()\n\npub discarded : File -> <IO> Unit\ndiscarded _ = ()\n";
    let shown = check_with_std_extras("", fs_extra, "");
    // どの診断も、すべてのラベルが Fs の中を指す
    for line in shown.lines().filter(|line| line.starts_with("  ")) {
        assert!(line.starts_with("  <std>/Fs.em "), "{shown}");
    }
    for code in ["E3002", "E3003", "E3004"] {
        assert!(shown.contains(code), "{code}\n{shown}");
    }
}

#[test]
fn a_carry_over_through_a_composition_points_into_the_prelude() {
    // `>>` の本体は `g` を持ったまま `f` を呼ぶので、線形な `g` を `multi` の `f` と合成すると E3006 になる
    let text = "chooser : Unit -> <Choice> Unit\nchooser () =\n  let b = choose ()\n  ()\n\ncomposed : Unit -> <Choice, IO> Unit\ncomposed () =\n  let h = Fs.open \"a.txt\"\n  let k = chooser >> (fn u -> Fs.close h)\n  k ()";
    let shown = check_with_prelude(CHOICE, text);
    assert!(shown.starts_with("E3006 "), "{shown}");
    assert!(shown.contains("  test.em \">>\" "), "{shown}");
    assert!(shown.contains("  <std>/Prelude.em "), "{shown}");
}

/// 入口と依存先のモジュールを検査し、推論結果と診断を並べる。
fn check_modules(entry: &str, modules: &[(&str, &str)]) -> String {
    let checked = eml_test_support::check_files(entry, modules);
    eml_test_support::with_diagnostics(
        eml_types::dump(&checked.program, &checked.typed),
        &eml_test_support::full(checked.files(), &checked.diagnostics),
    )
}

#[test]
fn the_dump_shows_every_module_but_the_prelude() {
    let entry = "import Report\n\nmain : Unit -> <IO> Unit\nmain () = println (Report.render (Report.parse \"x\"))";
    let report = "pub data Row = | Row Int\n\npub parse : String -> Row\nparse s = Row 1\n\npub render : Row -> String\nrender r = match r with\n  | Row n -> show n";
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
fn a_user_unit_is_told_apart_from_the_builtin_one() {
    // 空のレコードは Prelude の `Unit` の表示名で書く。継続は普通の関数型で表示するので、ユーザーの `Cont` は修飾しない
    let text = "data Unit = | U\n\ndata Cont = | C\n\neffect Ask where\n  ask : Int -> Int\n\nf : Cont -> Unit\nf c = ()\n\ng : Int -> Int\ng n =\n  handle ask n with\n    | ask x k -> k x";
    insta::assert_snapshot!(crate::common::check_text(text), @"
    ask : Int -> <Ask> Int
    f : Cont -> Main.Unit
      c#0 : Cont
    g : Int -> Int
      n#0 : Int
      x#1 : Int
      k#2 : Int -> Int
      $r#3 : Int
    ---
    E2001 9:7 mismatched types
      9:7 expected `Main.Unit`, found `Prelude.Unit`
      8:5 expected because of the signature of `f`
    ");
}

#[test]
fn notes_name_builtin_types_by_their_display_names() {
    // E2004、E2006 と型の不一致の注記も、Prelude の型を表示名で書く。ユーザーが `Bool` と `Unit` を定義すると、
    // Prelude のほうは `Prelude.Bool` と `Prelude.Unit` になる
    let text = "data Bool = | Yes | No\n\ndata Unit = | U\n\nmain : Unit -> <IO> Prelude.Unit\nmain u = ()\n\ncondition : Bool -> Int\ncondition b = if b then 1 else 0\n\ncompare : Bool -> Prelude.Bool\ncompare b = b == b\n\nwithout_else : Prelude.Bool -> Prelude.Unit\nwithout_else b = if b then 1\n\nstatement : Int -> Int\nstatement n =\n  n\n  n\n\nunit_pattern : Int -> Int\nunit_pattern () = 1";
    let checked = eml_test_support::check(text);
    insta::assert_snapshot!(eml_test_support::full(checked.files(), &checked.diagnostics), @"
    E2004 5:8 `main` must have type `Prelude.Unit -> <IO> Prelude.Unit`
      5:8 found `Main.Unit -> <IO> Prelude.Unit`
    E2001 9:18 mismatched types
      9:18 expected `Prelude.Bool`, found `Main.Bool`
      note: the condition of `if` must have type `Prelude.Bool`
    E2006 12:15 no instance of `Eq` for `Main.Bool`
      12:15 `==` requires `Eq Main.Bool`
    E2001 15:28 mismatched types
      15:28 expected `Prelude.Unit`, found `Int`
      note: an `if` without `else` must have type `Prelude.Unit`
    E2001 19:3 mismatched types
      19:3 expected `Prelude.Unit`, found `Int`
      note: a statement that is not the last one in a block must have type `Prelude.Unit`
    E2001 23:14 mismatched types
      23:14 expected `Int`, found `Prelude.Unit`
      note: the pattern `()` matches only `Prelude.Unit`
    ");
}

/// 2つのモジュールで同じ種類の構文が同じバイトの範囲にあっても、それぞれのモジュールの構文木から引く。
#[test]
fn each_module_reads_its_own_syntax() {
    // 2行目のシグネチャは、どちらのファイルでもバイト 9 から 27 にある
    let entry = "import A\npub h : Int -> Int\nh x = x + 1\n";
    let module = "-- 34567\npub k : a   ->   a\nk x = x\n";
    let checked = eml_test_support::check_files(entry, &[("A.em", module)]);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
}
