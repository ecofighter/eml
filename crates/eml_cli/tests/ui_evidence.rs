//! S7 の開発中だけの差分テスト。EP を通す経路と通さない経路で、run と run-fail の出力が一致することを確かめる
//! (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md の「差分テストと段の終わり」)。

use std::fs;
use std::path::{Path, PathBuf};

use eml_cli::{Compiled, OutputSink, RunConfig};

use crate::ui::{Entry, classify, load, ui_root};

/// 標準出力と、実行時エラーの表示。RC の回数と `RunStats` は経路で違うので比べない。
fn outcome(compiled: Compiled, dir: &Path) -> (String, Result<(), String>) {
    let program = compiled.program.expect("the test compiles");
    let (sink, captured) = OutputSink::capture();
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_file_root(dir.to_path_buf());
    let result = eml_cli::execute(&program, &config, sink)
        .map(|_| ())
        .map_err(|error| error.to_string());
    (captured.contents(), result)
}

/// `tests/ui/<top>` の下のテストの入口と、その接尾辞。
fn tests(top: &str) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    let mut pending = vec![ui_root().join(top)];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "em")
                && let Entry::Test(suffix) = classify(&path, top)
            {
                found.push((path, suffix));
            }
        }
    }
    found.sort();
    found
}

#[test]
fn every_run_test_has_the_same_output_with_evidence_passing() {
    for top in ["run", "run-fail"] {
        let found = tests(top);
        // テストを1つも見つけなければ、何も比べずに通ってしまう
        assert!(!found.is_empty(), "no tests under {top}");
        for (path, suffix) in found {
            let session = load(&path, &suffix);
            let dir = path.parent().expect("a test file has a directory");
            let passed = outcome(session.compile(), dir);
            let direct = outcome(session.compile_without_evidence(), dir);
            assert_eq!(passed, direct, "{top}/{suffix}");
        }
    }
}
