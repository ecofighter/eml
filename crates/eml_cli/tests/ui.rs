//! 成功すべきか失敗すべきかはディレクトリ (`run/`、`check-fail/`) で決める。スナップショットの承認を誤っても、
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

#[test]
fn run() {
    insta::glob!("../../../tests/ui", "run/*.em", |path| {
        let (files, id) = load(path);
        let mut config = RunConfig::default();
        config.debug_heap = true;
        let compiled = eml_cli::compile(&files, id);
        let stderr = render(&compiled.diagnostics, &files);
        let program = compiled
            .program
            .unwrap_or_else(|| panic!("unexpected errors:\n{stderr}"));
        let (sink, buffer) = OutputSink::capture();
        assert_eq!(
            eml_cli::execute(program, &config, sink),
            RunResult::Completed,
            "{stderr}"
        );
        let stdout = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        insta::assert_snapshot!(format!("--- stdout ---\n{stdout}--- stderr ---\n{stderr}"));
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
