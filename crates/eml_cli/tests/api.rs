use eml_cli::{OutputSink, RunConfig, Session, execute};
use eml_diagnostics::has_errors;

#[test]
fn compile_returns_no_program_when_there_are_errors() {
    let mut session = Session::new();
    let file = session.add_file("a.em", "€");
    let compiled = session.compile(file);
    assert!(has_errors(&compiled.diagnostics));
    assert!(compiled.program.is_none());
}

#[test]
fn compile_returns_a_program_without_errors() {
    let mut session = Session::new();
    let file = session.add_file("a.em", "main : Unit -> <IO> Unit\nmain () = ()");
    let compiled = session.compile(file);
    assert!(compiled.diagnostics.is_empty());
    assert!(compiled.program.is_some());
}

#[test]
fn execute_runs_a_compiled_program() {
    let mut session = Session::new();
    let file = session.add_file("a.em", "main : Unit -> <IO> Unit\nmain () = ()");
    let program = session.compile(file).program.unwrap();
    let (sink, _) = OutputSink::capture();
    assert_eq!(execute(program, &RunConfig::default(), sink), Ok(()));
}

#[test]
fn compile_reports_a_missing_main() {
    let mut session = Session::new();
    let file = session.add_file("a.em", "f : Int -> Int\nf x = x");
    let compiled = session.compile(file);
    let codes: Vec<String> = compiled
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(codes, ["E2003"]);
    assert!(compiled.program.is_none());
}

#[test]
fn check_accepts_a_file_without_main() {
    let mut session = Session::new();
    let file = session.add_file("a.em", "f : Int -> Int\nf x = x");
    assert!(session.check(file).is_empty());
}

#[test]
fn a_session_registers_the_prelude_for_rendering() {
    let mut session = Session::new();
    let file = session.add_file("a.em", "main : Unit -> <IO> Unit\nmain () = println \"x\"");
    assert!(session.check(file).is_empty());
    let prelude = session.prelude();
    assert_eq!(session.files().path(prelude), eml_hir::PRELUDE_PATH);
    assert_eq!(session.files().text(prelude), eml_hir::PRELUDE_SOURCE);
    // Prelude の範囲を指す診断を表示しても panic しない
    let label =
        eml_diagnostics::Label::new(prelude, eml_diagnostics::TextRange::empty(0.into()), "here");
    let diagnostic =
        eml_diagnostics::Diagnostic::error(eml_diagnostics::NOT_YET_SUPPORTED, "test", label);
    let rendered = eml_diagnostics::render(&[diagnostic], session.files());
    assert!(rendered.contains("Prelude.em"));
}
