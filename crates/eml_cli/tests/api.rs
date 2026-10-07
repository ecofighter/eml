use eml_cli::{
    FsProvider, ModulePath, ModuleSource, OutputSink, ReadError, RunConfig, Session, execute,
};
use eml_diagnostics::{Diagnostic, has_errors};
use eml_test_support::MemorySource;

use crate::common::temp_project;

/// import のない入口だけのセッション。
fn single(text: &str) -> Session {
    Session::load("a.em", text, &MemorySource(&[]))
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics.iter().map(|d| d.code.to_string()).collect()
}

fn module_path(segments: &[&str]) -> ModulePath {
    ModulePath(segments.iter().map(|segment| segment.to_string()).collect())
}

#[test]
fn compile_returns_no_program_when_there_are_errors() {
    let compiled = single("€").compile();
    assert!(has_errors(&compiled.diagnostics));
    assert!(compiled.program.is_none());
}

#[test]
fn compile_returns_a_program_without_errors() {
    let compiled = single("main : Unit -> <IO> Unit\nmain () = ()").compile();
    assert!(compiled.diagnostics.is_empty());
    assert!(compiled.program.is_some());
}

#[test]
fn execute_runs_a_compiled_program() {
    let program = single("main : Unit -> <IO> Unit\nmain () = ()")
        .compile()
        .program
        .unwrap();
    let (sink, _) = OutputSink::capture();
    assert_eq!(execute(program, &RunConfig::default(), sink), Ok(()));
}

#[test]
fn compile_reports_a_missing_main() {
    let compiled = single("f : Int -> Int\nf x = x").compile();
    assert_eq!(codes(&compiled.diagnostics), ["E2003"]);
    assert!(compiled.program.is_none());
}

#[test]
fn check_accepts_a_file_without_main() {
    assert!(single("f : Int -> Int\nf x = x").check().is_empty());
}

#[test]
fn a_session_registers_the_prelude_for_rendering() {
    let session = single("main : Unit -> <IO> Unit\nmain () = println \"x\"");
    assert!(session.check().is_empty());
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

#[test]
fn the_prelude_alone_has_no_diagnostics() {
    assert!(single("").check().is_empty());
}

#[test]
fn a_session_registers_a_dependency_under_the_entry_directory() {
    let source = MemorySource(&[("Report/Csv.em", "x : Int\nx = y")]);
    let session = Session::load("app/main.em", "import Report.Csv\n", &source);
    let diagnostics = session.check();
    assert_eq!(codes(&diagnostics), ["E1001"]);
    assert_eq!(
        session.files().path(diagnostics[0].primary.file),
        "app/Report/Csv.em"
    );
    assert_eq!(session.files().path(session.entry()), "app/main.em");
}

#[test]
fn a_missing_main_points_at_the_entry() {
    let source = MemorySource(&[("Util.em", "pub x : Int\nx = 1")]);
    let session = Session::load("app/main.em", "import Util\n", &source);
    let compiled = session.compile();
    assert_eq!(codes(&compiled.diagnostics), ["E2003"]);
    assert_eq!(compiled.diagnostics[0].primary.file, session.entry());
}

#[test]
fn fs_provider_reads_a_module_under_the_root() {
    let dir = temp_project("fs-read", &[("Report/Csv.em", "pub x : Int\nx = 1\n")]);
    let read = FsProvider::new(&dir).read(&module_path(&["Report", "Csv"]));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(read, Ok("pub x : Int\nx = 1\n".to_string()));
}

#[test]
fn fs_provider_matches_the_case_of_every_segment() {
    let dir = temp_project("fs-case", &[("report/Csv.em", ""), ("Util/csv.em", "")]);
    let provider = FsProvider::new(&dir);
    let directory = provider.read(&module_path(&["Report", "Csv"]));
    let file = provider.read(&module_path(&["Util", "Csv"]));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(directory, Err(ReadError::NotFound));
    assert_eq!(file, Err(ReadError::NotFound));
}

#[test]
fn a_module_in_a_differently_cased_directory_is_not_found() {
    let dir = temp_project("case", &[("report/Csv.em", "pub x : Int\nx = 1\n")]);
    let session = Session::load("main.em", "import Report.Csv\n", &FsProvider::new(&dir));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(codes(&session.check()), ["E1026"]);
}

#[test]
fn a_dependency_that_is_not_utf8_cannot_be_read() {
    let dir = temp_project("utf8", &[]);
    std::fs::write(dir.join("Bad.em"), [0x66, 0x6e, 0xff, 0xfe]).unwrap();
    let session = Session::load("main.em", "import Bad\n", &FsProvider::new(&dir));
    std::fs::remove_dir_all(&dir).unwrap();
    let diagnostics = session.check();
    assert_eq!(codes(&diagnostics), ["E1026"]);
    assert!(
        diagnostics[0]
            .message
            .contains("stream did not contain valid UTF-8"),
        "{}",
        diagnostics[0].message
    );
}

#[test]
fn a_directory_named_like_a_module_file_cannot_be_read() {
    let dir = temp_project("dir-module", &[("Csv.em/x.em", "")]);
    let session = Session::load("main.em", "import Csv\n", &FsProvider::new(&dir));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(codes(&session.check()), ["E1026"]);
}

#[test]
fn files_that_are_not_imported_are_not_read() {
    // 根の下の .em をすべて読むのではなく、import をたどる (docs/spec/modules.md)
    let dir = temp_project("unrelated", &[("Broken.em", "€")]);
    let session = Session::load("main.em", "f : Int\nf = 1\n", &FsProvider::new(&dir));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(session.check().is_empty());
}
