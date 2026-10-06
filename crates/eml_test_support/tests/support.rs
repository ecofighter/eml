use eml_core_ir::{Pass, pretty};
use eml_diagnostics::{Diagnostic, ErrorCode, Label, TextRange};
use eml_test_support::{
    check, core, core_until, full, lower, lower_clean, parse, parse_clean, run, short, short_text,
    source, with_diagnostics,
};

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

#[test]
fn core_until_stops_after_the_named_pass() {
    // `simplify` は、値を返すだけの join point を消す
    let joined = "pick : Bool -> Int\npick c =\n  let y = if c then 1 else 2\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (pick True))";
    assert!(pretty(&core_until(joined, Pass::Translate)).contains("join j0"));
    assert!(!pretty(&core_until(joined, Pass::Simplify)).contains("join"));
    // 途中で止めても、パスの後の処理 (`captures` の埋め直しと検査) を行う
    let captured = "pick : Bool -> String -> String\npick b s =\n  let t = if b then s else \"none\"\n  t ++ s\n\nmain : Unit -> <IO> Unit\nmain () = println (pick True \"x\")";
    let translated = pretty(&core_until(captured, Pass::Translate));
    assert!(translated.contains(") [s1] {"), "{translated}");
    // `s` を2回使うので、Perceus の後にだけ `dup` が入る
    let twice = "twice : String -> String\ntwice s = s ++ s\n\nmain : Unit -> <IO> Unit\nmain () = println (twice \"x\")";
    assert!(!pretty(&core_until(twice, Pass::Translate)).contains("dup"));
    assert!(!pretty(&core_until(twice, Pass::Simplify)).contains("dup"));
    assert!(pretty(&core_until(twice, Pass::Perceus)).contains("dup s0"));
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

#[test]
#[should_panic(expected = "unexpected diagnostics")]
fn lower_clean_rejects_an_undefined_name() {
    lower_clean("f : Int -> Int\nf x = g x");
}
