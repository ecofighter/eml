//! 成功すべきか失敗すべきかは最上位のディレクトリ (`run/`、`run-fail/`、`check-fail/`) で決める。スナップショットの承認を
//! 誤っても、成功と失敗の入れ替わりを検出できるようにするため。テストはその下の分類のサブディレクトリに置き、1つの
//! ファイルか、`main.em` を入口とする1つのディレクトリである (docs/implementation/testing.md の「UI テスト」)。

use std::fs;
use std::path::{Path, PathBuf};

use eml_cli::{FsProvider, OutputSink, RunConfig, RuntimeError, Session};
use eml_diagnostics::{has_errors, render};

fn ui_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/ui")
        .canonicalize()
        .unwrap()
}

/// スナップショットの中のパスを安定させるため、入口を `tests/ui` からの相対パスで渡す。モジュールの根は入口の
/// ディレクトリである。
fn load(path: &Path, suffix: &str) -> Session {
    let relative = path
        .strip_prefix(ui_root())
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let root = path.parent().expect("a test file has a directory");
    let session = Session::load(
        &relative,
        &fs::read_to_string(path).unwrap(),
        &FsProvider::new(root),
    );
    // ディレクトリのテストの接尾辞は `.em` で終わらない (`classify`)
    if suffix.ends_with(".em") {
        assert_single_file(&session, suffix);
    }
    session
}

/// 単独のファイルのテストは import を書かない。分類のディレクトリにある別のテストのファイルを、モジュールとして読んで
/// しまうため (docs/implementation/testing.md の「UI テスト」)。
fn assert_single_file(session: &Session, suffix: &str) {
    let modules: Vec<&str> = session.module_names().collect();
    assert!(
        modules.len() <= 2,
        "{suffix} imports {}: a test that imports modules must be a directory with main.em",
        modules[2..].join(", ")
    );
}

/// glob に当たった `.em` の役割。
#[derive(Debug, PartialEq)]
enum Entry {
    /// テストの入口。スナップショットの名前の接尾辞を持つ。
    Test(String),
    /// ディレクトリのテストの `main.em` から読むモジュール。単独のテストにしない。
    Module,
}

/// 置き場所の規則を確かめて、`.em` の役割を決める。成功や失敗の確かめより先に呼び、置き場所の誤りを先に伝える。
/// 接尾辞は最上位のディレクトリからの相対パス (`basics/hello.em`。ディレクトリのテストは `names/import_cycle`) にする。
/// insta の既定はすべての一致に共通の接頭辞を除くので、分類が1つしかないディレクトリでは名前に分類が入らず、分類が
/// 増えたときに名前が変わるため。
fn classify(path: &Path, top: &str) -> Entry {
    let top_dir = ui_root().join(top);
    let relative = path.strip_prefix(&top_dir).unwrap();
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    match parts.as_slice() {
        [category, file] => {
            // `main.em` はディレクトリのテストの入口の名前なので、分類の直下にあれば置き場所の誤りである
            assert!(
                file != "main.em",
                "tests/ui/{top}/{category}/main.em must be in a test directory: tests/ui/{top}/{category}/<name>/main.em"
            );
            Entry::Test(format!("{category}/{file}"))
        }
        [category, name, file] if file == "main.em" => Entry::Test(format!("{category}/{name}")),
        [category, name, _, ..] => {
            // `main.em` を書き忘れたディレクトリのモジュールが、単独のテストとして黙って通らないようにする
            assert!(
                top_dir.join(category).join(name).join("main.em").is_file(),
                "{} belongs to no test: add tests/ui/{top}/{category}/{name}/main.em",
                relative.display()
            );
            Entry::Module
        }
        _ => panic!(
            "{} must be in a category directory under tests/ui/{top}/",
            relative.display()
        ),
    }
}

fn snapshot_settings(suffix: &str) -> insta::Settings {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_suffix(suffix);
    settings
}

/// 診断のエラーなしでコンパイルし、`debug_heap` を有効にして実行する。stdout、診断の表示、実行の結果を返す。
fn compile_and_execute(path: &Path, suffix: &str) -> (String, String, Result<(), RuntimeError>) {
    let session = load(path, suffix);
    let compiled = session.compile();
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
        let Entry::Test(suffix) = classify(path, "run") else {
            return;
        };
        let (stdout, stderr, result) = compile_and_execute(path, &suffix);
        assert_eq!(result, Ok(()), "{stderr}");
        snapshot_settings(&suffix).bind(|| {
            insta::assert_snapshot!(format!("--- stdout ---\n{stdout}--- stderr ---\n{stderr}"));
        });
    });
}

#[test]
fn run_fail() {
    insta::glob!("../../../tests/ui", "run-fail/**/*.em", |path| {
        let Entry::Test(suffix) = classify(path, "run-fail") else {
            return;
        };
        let (stdout, _, result) = compile_and_execute(path, &suffix);
        let Err(error) = result else {
            panic!("expected a runtime error");
        };
        snapshot_settings(&suffix).bind(|| {
            insta::assert_snapshot!(format!(
                "--- stdout ---\n{stdout}--- runtime error ---\n{error}\n"
            ));
        });
    });
}

#[test]
fn check_fail() {
    insta::glob!("../../../tests/ui", "check-fail/**/*.em", |path| {
        let Entry::Test(suffix) = classify(path, "check-fail") else {
            return;
        };
        let session = load(path, &suffix);
        let diagnostics = session.check();
        let rendered = render(&diagnostics, session.files());
        assert!(
            has_errors(&diagnostics),
            "expected at least one error, got:\n{rendered}"
        );
        snapshot_settings(&suffix).bind(|| {
            insta::assert_snapshot!(rendered);
        });
    });
}

#[test]
fn the_layout_decides_what_each_file_is() {
    let root = ui_root();
    assert_eq!(
        classify(&root.join("run/basics/hello.em"), "run"),
        Entry::Test("basics/hello.em".to_string())
    );
    assert_eq!(
        classify(&root.join("run/modules/qualified/main.em"), "run"),
        Entry::Test("modules/qualified".to_string())
    );
    assert_eq!(
        classify(&root.join("run/modules/qualified/Report/Csv.em"), "run"),
        Entry::Module
    );
}

#[test]
#[should_panic(expected = "must be in a category directory")]
fn a_file_right_under_the_top_directory_fails() {
    classify(&ui_root().join("run/hello.em"), "run");
}

#[test]
#[should_panic(expected = "must be in a test directory")]
fn main_right_under_a_category_fails() {
    classify(&ui_root().join("run/modules/main.em"), "run");
}

#[test]
#[should_panic(expected = "belongs to no test")]
fn a_module_without_main_fails() {
    classify(&ui_root().join("run/modules/no_such_test/Lib.em"), "run");
}

#[test]
#[should_panic(expected = "basics/imports.em imports Util")]
fn a_single_file_test_that_imports_a_module_fails() {
    let source = eml_test_support::MemorySource(&[("Util.em", "")]);
    let session = Session::load("run/basics/imports.em", "import Util\n", &source);
    assert_single_file(&session, "basics/imports.em");
}
