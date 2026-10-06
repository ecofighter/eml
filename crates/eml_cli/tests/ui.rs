//! 成功すべきか失敗すべきかは最上位のディレクトリ (`run/`、`run-fail/`、`check-fail/`) で決める。スナップショットの承認を
//! 誤っても、成功と失敗の入れ替わりを検出できるようにするため。テストはその下の分類のサブディレクトリに置く
//! (docs/implementation/testing.md の「UI テスト」)。

use std::fs;
use std::path::{Path, PathBuf};

use eml_cli::{OutputSink, RunConfig, RuntimeError, Session};
use eml_diagnostics::{has_errors, render};

fn ui_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/ui")
        .canonicalize()
        .unwrap()
}

/// スナップショットの中のパスを安定させるため、`tests/ui` からの相対パスでファイルを登録する。
fn load(path: &Path) -> (Session, eml_diagnostics::FileId) {
    let relative = path
        .strip_prefix(ui_root())
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let mut session = Session::new();
    let id = session.add_file(relative, fs::read_to_string(path).unwrap());
    (session, id)
}

/// スナップショットの名前を、最上位のディレクトリからの相対パス (`basics/hello.em`) で固定する。insta の既定はすべての
/// 一致に共通の接頭辞を除くので、分類が1つしかないディレクトリでは名前に分類が入らず、分類が増えたときに名前が変わる。
/// 直下の .em は分類の規則に反するので拒む。成功や失敗の確かめより先に呼び、置き場所の誤りを先に伝える。
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
    let (session, id) = load(path);
    let compiled = session.compile(id);
    let stderr = render(&compiled.diagnostics, session.files());
    let program = compiled
        .program
        .unwrap_or_else(|| panic!("unexpected errors:\n{stderr}"));
    let (sink, captured) = OutputSink::capture();
    // 入力のファイルはテストの隣に置く (docs/implementation/testing.md の「UI テスト」)
    let root = path
        .parent()
        .expect("a test file has a directory")
        .to_path_buf();
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_file_root(root);
    let result = eml_cli::execute(program, &config, sink);
    (captured.contents(), stderr, result)
}

#[test]
fn run() {
    insta::glob!("../../../tests/ui", "run/**/*.em", |path| {
        let settings = categorized(path, "run");
        let (stdout, stderr, result) = compile_and_execute(path);
        assert_eq!(result, Ok(()), "{stderr}");
        settings.bind(|| {
            insta::assert_snapshot!(format!("--- stdout ---\n{stdout}--- stderr ---\n{stderr}"));
        });
    });
}

#[test]
fn run_fail() {
    insta::glob!("../../../tests/ui", "run-fail/**/*.em", |path| {
        let settings = categorized(path, "run-fail");
        let (stdout, _, result) = compile_and_execute(path);
        let Err(error) = result else {
            panic!("expected a runtime error");
        };
        settings.bind(|| {
            insta::assert_snapshot!(format!(
                "--- stdout ---\n{stdout}--- runtime error ---\n{error}\n"
            ));
        });
    });
}

#[test]
fn check_fail() {
    insta::glob!("../../../tests/ui", "check-fail/**/*.em", |path| {
        let settings = categorized(path, "check-fail");
        let (session, id) = load(path);
        let diagnostics = session.check(id);
        let rendered = render(&diagnostics, session.files());
        assert!(
            has_errors(&diagnostics),
            "expected at least one error, got:\n{rendered}"
        );
        settings.bind(|| {
            insta::assert_snapshot!(rendered);
        });
    });
}
