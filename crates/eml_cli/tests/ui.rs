//! 成功すべきか失敗すべきかは最上位のディレクトリ (`run/`、`run-fail/`、`check-fail/`) で決める。スナップショットの承認を
//! 誤っても、成功と失敗の入れ替わりを検出できるようにするため。テストはその下の分類のサブディレクトリに置く
//! (docs/implementation/testing.md の「UI テスト」)。

use std::fs;
use std::path::{Path, PathBuf};

use eml_cli::{OutputSink, RunConfig, RuntimeError};
use eml_diagnostics::{SourceFiles, has_errors, render};

fn ui_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/ui")
        .canonicalize()
        .unwrap()
}

/// スナップショットの中のパスを安定させるため、`tests/ui` からの相対パスでファイルを登録する。
fn load(path: &Path) -> (SourceFiles, eml_diagnostics::FileId) {
    let relative = path
        .strip_prefix(ui_root())
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let mut files = SourceFiles::new();
    let id = files.add(relative, fs::read_to_string(path).unwrap());
    (files, id)
}

/// スナップショットの名前を、最上位のディレクトリからの相対パス (`basics/hello.em`) で固定する。insta の既定はすべての
/// 一致に共通の接頭辞を除くので、分類が1つしかないディレクトリでは名前に分類が入らず、分類が増えたときに名前が変わる。
/// 直下の .em は分類の規則に反するので拒む。
fn categorized(path: &Path, top: &str) -> insta::Settings {
    let relative = path.strip_prefix(ui_root().join(top)).unwrap();
    assert!(
        relative.components().count() >= 2,
        "{} must be in a category directory under tests/ui/{top}/",
        relative.display()
    );
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_suffix(relative.to_string_lossy().replace('\\', "/"));
    settings
}

/// 診断のエラーなしでコンパイルし、`debug_heap` を有効にして実行する。stdout、診断の表示、実行の結果を返す。
fn compile_and_execute(path: &Path) -> (String, String, Result<(), RuntimeError>) {
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
    insta::glob!("../../../tests/ui", "run/**/*.em", |path| {
        let (stdout, stderr, result) = compile_and_execute(path);
        assert_eq!(result, Ok(()), "{stderr}");
        categorized(path, "run").bind(|| {
            insta::assert_snapshot!(format!("--- stdout ---\n{stdout}--- stderr ---\n{stderr}"));
        });
    });
}

#[test]
fn run_fail() {
    insta::glob!("../../../tests/ui", "run-fail/**/*.em", |path| {
        let (stdout, _, result) = compile_and_execute(path);
        let Err(error) = result else {
            panic!("expected a runtime error");
        };
        categorized(path, "run-fail").bind(|| {
            insta::assert_snapshot!(format!(
                "--- stdout ---\n{stdout}--- runtime error ---\n{error}\n"
            ));
        });
    });
}

#[test]
fn check_fail() {
    insta::glob!("../../../tests/ui", "check-fail/**/*.em", |path| {
        let (files, id) = load(path);
        let diagnostics = eml_cli::check(&files, id);
        let rendered = render(&diagnostics, &files);
        assert!(
            has_errors(&diagnostics),
            "expected at least one error, got:\n{rendered}"
        );
        categorized(path, "check-fail").bind(|| {
            insta::assert_snapshot!(rendered);
        });
    });
}
