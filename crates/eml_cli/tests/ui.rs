//! 成功すべきか失敗すべきかは最上位のディレクトリ (`run/`、`run-fail/`、`check-fail/`) で決める。スナップショットの承認を
//! 誤っても、成功と失敗の入れ替わりを検出できるようにするため。テストはその下の分類のサブディレクトリに置き、1つの
//! ファイルか、`main.em` を入口とする1つのディレクトリである (docs/implementation/testing.md の「UI テスト」)。

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eml_cli::{
    CaptureHost, Control, FsProvider, RunConfig, RuntimeError, Session, VmFault, default_limits,
    execute,
};
use eml_diagnostics::{has_errors, render};
use eml_rt::Fault;

use crate::common::{assert_filter_matches, for_each_file, run_all, temp_project};

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

/// 単独のファイルのテストは、ユーザーのモジュールを import しない。分類のディレクトリにある別のテストのファイルを、
/// モジュールとして読んでしまうため (docs/implementation/testing.md の「UI テスト」)。標準ライブラリのモジュールは
/// いつも読み込むので数えない。
fn assert_single_file(session: &Session, suffix: &str) {
    let modules: Vec<&str> = session.user_module_names().collect();
    assert!(
        modules.len() <= 1,
        "{suffix} imports {}: a test that imports modules must be a directory with main.em",
        modules[1..].join(", ")
    );
}

/// 集めた `.em` の役割。
#[derive(Debug, PartialEq)]
enum Entry {
    /// テストの入口。スナップショットの名前の接尾辞を持つ。
    Test(String),
    /// ディレクトリのテストの `main.em` から読むモジュール。単独のテストにしない。
    Module,
}

/// 置き場所の規則を確かめて、`.em` の役割を決める。成功や失敗の確かめより先に呼び、置き場所の誤りを先に伝える。
/// 接尾辞は最上位のディレクトリからの相対パス (`basics/hello.em`。ディレクトリのテストは `names/import_cycle`) にする。
/// 名前がテストの置き場所だけで決まり、ほかのテストを足しても変わらないため。
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

fn snapshot_settings(path: &Path, suffix: &str) -> insta::Settings {
    let mut settings = insta::Settings::clone_current();
    settings.set_input_file(path);
    settings.set_snapshot_suffix(suffix);
    settings
}

/// 診断のエラーなしでコンパイルし、LIR と、入力のファイルを置くディレクトリと、診断の表示を返す。入力のファイルは
/// テストの隣に置く (docs/implementation/testing.md の「UI テスト」)。
fn compile(path: &Path, suffix: &str) -> (eml_lir::Program, PathBuf, String) {
    let session = load(path, suffix);
    let compiled = session.compile();
    let stderr = render(&compiled.diagnostics, session.files());
    let program = compiled
        .program
        .unwrap_or_else(|| panic!("unexpected errors:\n{stderr}"));
    let root = path
        .parent()
        .expect("a test file has a directory")
        .to_path_buf();
    (program, root, stderr)
}

/// 実行の結果。`stderr` はプログラムと子のプロセスが標準エラーに書いたものである。`written` は `Fs.write_text` が
/// 書いたファイルを、スナップショットに足す形に並べたものである。`result` の `Ok` は `exit` の終了コードである。
struct Ran {
    stdout: String,
    stderr: String,
    written: String,
    result: Result<Option<i64>, RuntimeError>,
}

/// テストの入力のファイル `<名前>.em` の隣にある付属のファイルから、テストの Host を作る。`.args` は1行に1つの引数、
/// `.stdin` はそのままのバイト列、`.env` は `KEY=VALUE` の行で、どれも省ける (docs/implementation/testing.md の
/// 「UI テスト」)。
fn host_for(path: &Path, root: &Path) -> CaptureHost {
    let read = |extension: &str| fs::read(path.with_extension(extension)).ok();
    let lines = |bytes: Vec<u8>| -> Vec<String> {
        String::from_utf8(bytes)
            .expect("a side file is UTF-8")
            .lines()
            .map(str::to_string)
            .collect()
    };
    let mut host = CaptureHost::new(root.to_path_buf());
    if let Some(args) = read("args") {
        host = host.with_args(lines(args).into_iter().map(OsString::from).collect());
    }
    if let Some(stdin) = read("stdin") {
        host = host.with_stdin(stdin);
    }
    if let Some(env) = read("env") {
        let env = lines(env)
            .into_iter()
            .map(|line| {
                let (key, value) = line.split_once('=').expect("a `.env` line is KEY=VALUE");
                (key.to_string(), OsString::from(value))
            })
            .collect();
        host = host.with_env(env);
    }
    host
}

/// `eml run` と同じ既定の上限で流す。止める理由のビットは `Control::clear` でしか下りないので、実行ごとに新しい
/// `Control` を持つ設定を作る。使い回すと、前の実行で立ったビットで次の実行がすぐ止まる。
fn execute_in(program: &eml_lir::Program, path: &Path, root: &Path, debug_heap: bool) -> Ran {
    let host = host_for(path, root);
    let (out, err, files) = (host.output(), host.errors(), host.written());
    let config = RunConfig::default()
        .with_debug_heap(debug_heap)
        .with_control(Arc::new(Control::new(default_limits())));
    let result = execute(program, &config, host.into_ref()).map(|finished| finished.exit);
    let written = files
        .files()
        .into_iter()
        .map(|(path, text)| format!("--- written: {path} ---\n{text}"))
        .collect();
    Ran {
        stdout: out.contents(),
        stderr: err.contents(),
        written,
        result,
    }
}

/// 検査モードの結果をスナップショットと比べた後で、速いモードでも流し、同じ出力と同じ結果になることを確かめる。
/// 速いモードは前提を確かめないので、VM の誤りが未定義の動作になって同じプロセスのほかのファイルのテストを
/// 壊しうる。そのため検査モードの後に流し、検査モードが VM の誤りを見つけたときは流さない (`fast_mode_reports`。
/// docs/implementation/testing.md の「2つのモードで流す」)。
fn assert_fast_mode_agrees(
    suffix: &str,
    program: &eml_lir::Program,
    path: &Path,
    root: &Path,
    checked: &Ran,
) {
    let fast = execute_in(program, path, root, false);
    assert_eq!(
        fast.stdout, checked.stdout,
        "{suffix}: the fast mode printed something else"
    );
    assert_eq!(
        fast.stderr, checked.stderr,
        "{suffix}: the fast mode wrote another standard error"
    );
    assert_eq!(
        fast.written, checked.written,
        "{suffix}: the fast mode wrote other files"
    );
    assert_eq!(
        fast.result.map_err(|error| error.to_string()),
        checked
            .result
            .as_ref()
            .copied()
            .map_err(ToString::to_string),
        "{suffix}: the fast mode ended otherwise"
    );
}

/// 速いモードも同じ誤りを報告するか。漏れ、内部の誤り、ヒープの誤りは検査モードだけが見つける。これらは VM 自身の
/// 誤りを意味するので、速いモードで流すとメモリを壊すことがある。
fn fast_mode_reports(error: &RuntimeError) -> bool {
    !matches!(
        error,
        RuntimeError::Leak(_)
            | RuntimeError::Fault {
                fault: VmFault::Internal(_) | VmFault::Extern(Fault::Heap(_)),
                ..
            }
    )
}

#[test]
fn run() {
    for_each_file(&ui_root().join("run"), "em", |path| {
        let Entry::Test(suffix) = classify(path, "run") else {
            return;
        };
        let (program, root, diagnostics) = compile(path, &suffix);
        let ran = execute_in(&program, path, &root, true);
        assert!(ran.result.is_ok(), "{:?}\n{diagnostics}", ran.result);
        let (stdout, stderr, written) = (&ran.stdout, &ran.stderr, &ran.written);
        let exit = match ran.result {
            Ok(Some(code)) => format!("--- exit: {code} ---\n"),
            _ => String::new(),
        };
        snapshot_settings(path, &suffix).bind(|| {
            insta::assert_snapshot!(format!(
                "--- stdout ---\n{stdout}--- stderr ---\n{diagnostics}{stderr}{written}{exit}"
            ));
        });
        assert_fast_mode_agrees(&suffix, &program, path, &root, &ran);
    });
}

#[test]
fn run_fail() {
    for_each_file(&ui_root().join("run-fail"), "em", |path| {
        let Entry::Test(suffix) = classify(path, "run-fail") else {
            return;
        };
        let (program, root, diagnostics) = compile(path, &suffix);
        let ran = execute_in(&program, path, &root, true);
        let Err(error) = &ran.result else {
            panic!("expected a runtime error");
        };
        let (stdout, stderr, written) = (&ran.stdout, &ran.stderr, &ran.written);
        snapshot_settings(path, &suffix).bind(|| {
            insta::assert_snapshot!(format!(
                "--- stdout ---\n{stdout}--- stderr ---\n{diagnostics}{stderr}--- runtime error ---\n{error}\n{written}"
            ));
        });
        if fast_mode_reports(error) {
            assert_fast_mode_agrees(&suffix, &program, path, &root, &ran);
        }
    });
}

#[test]
fn check_fail() {
    for_each_file(&ui_root().join("check-fail"), "em", |path| {
        let Entry::Test(suffix) = classify(path, "check-fail") else {
            return;
        };
        let session = load(path, &suffix);
        let diagnostics = session.check().diagnostics;
        let rendered = render(&diagnostics, session.files());
        assert!(
            has_errors(&diagnostics),
            "expected at least one error, got:\n{rendered}"
        );
        snapshot_settings(path, &suffix).bind(|| {
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
fn a_single_file_test_may_use_the_standard_library() {
    let session = Session::load(
        "run/basics/hello.em",
        "",
        &eml_test_support::MemorySource(&[]),
    );
    assert!(session.module_names().count() > session.user_module_names().count());
    assert_single_file(&session, "basics/hello.em");
}

#[test]
#[should_panic(expected = "basics/imports.em imports Util")]
fn a_single_file_test_that_imports_a_module_fails() {
    let source = eml_test_support::MemorySource(&[("Util.em", "")]);
    let session = Session::load("run/basics/imports.em", "import Util\n", &source);
    assert_single_file(&session, "basics/imports.em");
}

#[test]
fn every_failing_file_is_reported() {
    let dir = temp_project(
        "for-each-file",
        &[("pass.em", ""), ("fail_a.em", ""), ("fail_b.em", "")],
    );
    let files = [
        dir.join("fail_a.em"),
        dir.join("fail_b.em"),
        dir.join("pass.em"),
    ];
    let result = std::panic::catch_unwind(|| {
        run_all(&files, |path| {
            assert!(!path.ends_with("fail_a.em") && !path.ends_with("fail_b.em"));
        });
    });
    fs::remove_dir_all(&dir).unwrap();
    let payload = result.expect_err("two files failed");
    let message = payload.downcast_ref::<String>().unwrap();
    assert!(message.contains("2 of 3 files failed"), "{message}");
    assert!(message.contains("fail_a.em: "), "{message}");
    assert!(message.contains("fail_b.em: "), "{message}");
    assert!(!message.contains("pass.em"), "{message}");
}

#[test]
#[should_panic(expected = "matches no UI test or transcript")]
fn a_filter_that_matches_nothing_fails() {
    assert_filter_matches("no-such-file");
}
