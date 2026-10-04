//! 成功すべきか失敗すべきかはディレクトリ (`run/`、`run-fail/`、`check-fail/`) で決める。スナップショットの承認を誤っても、
//! 成功と失敗の入れ替わりを検出できるようにするため (docs/implementation/testing.md)。

use std::fs;
use std::path::Path;

use eml_cli::{OutputSink, RunConfig, RunResult};
use eml_diagnostics::{SourceFiles, has_errors, render};

/// スナップショットの中のパスを安定させるため、`tests/ui` からの相対パスでファイルを登録する。
fn load(path: &Path) -> (SourceFiles, eml_diagnostics::FileId) {
    let ui_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/ui")
        .canonicalize()
        .unwrap();
    let relative = path
        .strip_prefix(&ui_root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let mut files = SourceFiles::new();
    let id = files.add(relative, fs::read_to_string(path).unwrap());
    (files, id)
}

/// 診断のエラーなしでコンパイルし、`debug_heap` を有効にして実行する。stdout、診断の表示、実行の結果を返す。
fn compile_and_execute(path: &Path) -> (String, String, RunResult) {
    let (files, id) = load(path);
    let compiled = eml_cli::compile(&files, id);
    let stderr = render(&compiled.diagnostics, &files);
    let program = compiled
        .program
        .unwrap_or_else(|| panic!("unexpected errors:\n{stderr}"));
    let (sink, captured) = OutputSink::capture();
    let config = RunConfig::default().with_debug_heap(true);
    let result = eml_cli::execute(program, &config, sink);
    (captured.contents(), stderr, result)
}

#[test]
fn run() {
    insta::glob!("../../../tests/ui", "run/*.em", |path| {
        let (stdout, stderr, result) = compile_and_execute(path);
        assert_eq!(result, RunResult::Completed, "{stderr}");
        insta::assert_snapshot!(format!("--- stdout ---\n{stdout}--- stderr ---\n{stderr}"));
    });
}

#[test]
fn run_fail() {
    insta::glob!("../../../tests/ui", "run-fail/*.em", |path| {
        let (stdout, _, result) = compile_and_execute(path);
        let RunResult::RuntimeError(message) = result else {
            panic!("expected a runtime error");
        };
        insta::assert_snapshot!(format!(
            "--- stdout ---\n{stdout}--- runtime error ---\n{message}\n"
        ));
    });
}

#[test]
fn check_fail() {
    insta::glob!("../../../tests/ui", "check-fail/*.em", |path| {
        let (files, id) = load(path);
        let diagnostics = eml_cli::check(&files, id);
        let rendered = render(&diagnostics, &files);
        assert!(
            has_errors(&diagnostics),
            "expected at least one error, got:\n{rendered}"
        );
        insta::assert_snapshot!(rendered);
    });
}
