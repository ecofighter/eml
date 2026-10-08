#[cfg(feature = "core")]
use eml_core_ir::{Pass, pretty};
use eml_diagnostics::{Diagnostic, ErrorCode, Label, TextRange};
#[cfg(feature = "hir")]
use eml_test_support::lower_clean;
#[cfg(feature = "types")]
use eml_test_support::{check, check_files, lower, lower_files, parse};
#[cfg(feature = "core")]
use eml_test_support::{core, core_until};
use eml_test_support::{full, parse_clean, short, short_text, source, with_diagnostics};
#[cfg(feature = "run")]
use eml_test_support::{run, run_files};

fn range(start: u32, end: u32) -> TextRange {
    TextRange::new(start.into(), end.into())
}

#[test]
fn short_puts_code_position_and_message_on_one_line() {
    let (files, file) = source("a\nbcd");
    let diagnostic = Diagnostic::error(ErrorCode(1), "bad", Label::new(file, range(3, 4), "here"));
    assert_eq!(short(&files, &[diagnostic]), ["E0001 2:2 bad"]);
}

#[test]
fn full_adds_labels_notes_and_help() {
    let (files, file) = source("ab\ncd");
    let diagnostic = Diagnostic::error(
        ErrorCode(2001),
        "bad",
        Label::new(file, range(1, 2), "here"),
    )
    .with_secondary(Label::new(file, range(3, 4), "there"))
    .with_note("a note")
    .with_help("a help");
    assert_eq!(
        full(&files, &[diagnostic]),
        "E2001 1:2 bad\n  1:2 here\n  2:1 there\n  note: a note\n  help: a help\n"
    );
}

#[cfg(feature = "types")]
#[test]
fn stages_collect_the_diagnostics_of_earlier_stages() {
    // `€` は字句の E0001、`g` は HIR の E1001 (未定義の名前) になる。
    let text = "€\nf : Int -> Int\nf x = g x";
    let codes = |diagnostics: &[Diagnostic]| -> Vec<String> {
        diagnostics.iter().map(|d| d.code.to_string()).collect()
    };
    assert_eq!(codes(&parse(text).diagnostics), ["E0001"]);
    assert_eq!(codes(&lower(text).diagnostics), ["E0001", "E1001"]);
    assert_eq!(codes(&check(text).diagnostics), ["E0001", "E1001"]);
}

#[cfg(feature = "types")]
#[test]
fn the_files_variants_read_modules_from_memory() {
    let entry = "import Util\n\nmain : Unit -> <IO> Unit\nmain () = println \"hi\"";
    let codes = |diagnostics: &[Diagnostic]| -> Vec<String> {
        diagnostics.iter().map(|d| d.code.to_string()).collect()
    };
    // 依存先の字句の誤りも、読み込みの段の診断として結果に入る
    let broken = [("Util.em", "€")];
    assert_eq!(codes(&lower_files(entry, &broken).diagnostics), ["E0001"]);
    assert_eq!(codes(&check_files(entry, &broken).diagnostics), ["E0001"]);
    #[cfg(feature = "run")]
    let clean = [("Util.em", "pub x : Int\nx = 1")];
    #[cfg(feature = "run")]
    assert_eq!(run_files(entry, &clean), ("hi\n".to_string(), Ok(())));
    let missing = lower_files("import Util", &[]);
    assert_eq!(
        short(missing.files(), &missing.diagnostics),
        ["E1026 1:8 cannot find module `Util`"]
    );
    // 入口以外のモジュールの `main` は、実行を始める関数でも E2004 の対象でもない (docs/spec/types.md)
    let library_main = [("Util.em", "pub main : Int\nmain = 1")];
    assert_eq!(
        codes(&check_files(entry, &library_main).diagnostics),
        Vec::<String>::new()
    );
    #[cfg(feature = "run")]
    assert_eq!(
        run_files(entry, &library_main),
        ("hi\n".to_string(), Ok(()))
    );
}

#[test]
#[cfg(feature = "run")]
fn run_executes_with_the_heap_checks() {
    let (stdout, result) = run("main : Unit -> <IO> Unit\nmain () = println \"hi\"");
    assert_eq!(stdout, "hi\n");
    assert_eq!(result, Ok(()));
}

#[cfg(feature = "core")]
#[test]
#[should_panic]
fn core_rejects_programs_with_errors() {
    core("f : Int -> Int\nf x = g x");
}

#[cfg(feature = "core")]
#[test]
fn core_until_stops_after_the_named_pass() {
    // contract は、使われない純粋な `let` を消す
    let unused = "f : Int -> Int\nf x =\n  let s = \"unused\"\n  x\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (f 1))";
    assert!(pretty(&core_until(unused, Pass::Translate)).contains("const \"unused\""));
    assert!(!pretty(&core_until(unused, Pass::Contract)).contains("\"unused\""));
    // `s` を2回使うので、Perceus の後にだけ `dup` が入る
    let twice = "twice : String -> String\ntwice s = s ++ s\n\nmain : Unit -> <IO> Unit\nmain () = println (twice \"x\")";
    assert!(!pretty(&core_until(twice, Pass::Translate)).contains("dup"));
    assert!(!pretty(&core_until(twice, Pass::Contract)).contains("dup"));
    assert!(pretty(&core_until(twice, Pass::Perceus)).contains("dup s.0"));
    assert_eq!(
        pretty(&core_until(twice, Pass::Perceus)),
        pretty(&core(twice))
    );
}

#[test]
fn with_diagnostics_leaves_a_dump_without_diagnostics_as_it_is() {
    assert_eq!(with_diagnostics("tree\n".to_string(), ""), "tree\n");
}

#[test]
fn with_diagnostics_separates_the_diagnostics_with_a_line() {
    assert_eq!(
        with_diagnostics("tree\n".to_string(), "E0001 1:1 bad\n"),
        "tree\n---\nE0001 1:1 bad\n"
    );
}

#[test]
fn short_text_ends_every_line_with_a_newline() {
    let (files, file) = source("a\nbcd");
    let diagnostics = [
        Diagnostic::error(ErrorCode(1), "bad", Label::new(file, range(0, 1), "here")),
        Diagnostic::error(ErrorCode(2), "worse", Label::new(file, range(3, 4), "here")),
    ];
    assert_eq!(
        short_text(&files, &diagnostics),
        "E0001 1:1 bad\nE0002 2:2 worse\n"
    );
    assert_eq!(short_text(&files, &[]), "");
}

#[test]
fn parse_clean_returns_a_tree_without_diagnostics() {
    let parsed = parse_clean("x = 1");
    assert!(parsed.diagnostics.is_empty());
    assert_eq!(parsed.parse.tree().items().count(), 1);
}

#[test]
#[should_panic(expected = "unexpected diagnostics")]
fn parse_clean_rejects_a_syntax_error() {
    parse_clean("x = €");
}

#[cfg(feature = "hir")]
#[test]
fn lower_clean_returns_a_module_without_diagnostics() {
    let lowered = lower_clean("f : Int -> Int\nf x = x");
    assert!(lowered.diagnostics.is_empty());
    assert!(
        lowered
            .program
            .functions()
            .any(|(_, function)| function.name == "f")
    );
}

#[cfg(feature = "hir")]
#[test]
#[should_panic(expected = "unexpected diagnostics")]
fn lower_clean_rejects_an_undefined_name() {
    lower_clean("f : Int -> Int\nf x = g x");
}
