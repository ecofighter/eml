use eml_cli::{OutputSink, RunConfig, RunResult, compile, execute};
use eml_diagnostics::{SourceFiles, has_errors};

#[test]
fn compile_returns_no_program_when_there_are_errors() {
    let mut files = SourceFiles::new();
    let file = files.add("a.em", "€");
    let compiled = compile(&files, file);
    assert!(has_errors(&compiled.diagnostics));
    assert!(compiled.program.is_none());
}

#[test]
fn compile_returns_a_program_without_errors() {
    let mut files = SourceFiles::new();
    let file = files.add("a.em", "main : Unit -> <IO> Unit\nmain () = ()");
    let compiled = compile(&files, file);
    assert!(compiled.diagnostics.is_empty());
    assert!(compiled.program.is_some());
}

#[test]
fn execute_runs_a_compiled_program() {
    let mut files = SourceFiles::new();
    let file = files.add("a.em", "main : Unit -> <IO> Unit\nmain () = ()");
    let program = compile(&files, file).program.unwrap();
    let (sink, _) = OutputSink::capture();
    assert_eq!(
        execute(program, &RunConfig::default(), sink),
        RunResult::Completed
    );
}
