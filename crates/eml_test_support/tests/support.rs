use eml_diagnostics::{Diagnostic, ErrorCode, Label, TextRange};
use eml_test_support::{check, core, full, lower, parse, run, short, source};

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

#[test]
fn run_executes_with_the_heap_checks() {
    let (stdout, result) = run("main : Unit -> <IO> Unit\nmain () = println \"hi\"");
    assert_eq!(stdout, "hi\n");
    assert_eq!(result, Ok(()));
}

#[test]
#[should_panic]
fn core_rejects_programs_with_errors() {
    core("f : Int -> Int\nf x = g x");
}
