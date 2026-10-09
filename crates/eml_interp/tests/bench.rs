//! 基準のプログラム (`bench/`) の実行の仕事の回数を固定する。回数は決定的なので、段の前後の回数をこのスナップショットの
//! 差分で残す (docs/implementation/benchmarks.md の「回数のテスト」)。

use std::path::{Path, PathBuf};

use eml_interp::RunStats;
use eml_test_support::run_stats;

/// テストを持つ基準のプログラム。`bench/` にプログラムを足したら、ここと下のテストの関数の両方に足す。
const PROGRAMS: [&str; 6] = ["empty", "fib", "list", "loop", "state", "tree"];

fn bench_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench")
}

/// `debug_heap` 付きで走らせ、出力を確かめてから回数を返す。リークと実行時エラーは panic にする。
fn stats(name: &str, expected: &str) -> RunStats {
    let path = bench_dir().join(format!("{name}.em"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let (out, result) = run_stats(&text);
    assert_eq!(out, expected);
    result.unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn every_program_in_bench_has_a_test() {
    let mut names: Vec<String> = std::fs::read_dir(bench_dir())
        .expect("bench/ exists")
        .map(|entry| entry.expect("a readable entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "em"))
        .map(|path| {
            path.file_stem()
                .expect("a file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert_eq!(names, PROGRAMS);
}

#[test]
fn empty() {
    insta::assert_debug_snapshot!(stats("empty", ""), @"
    RunStats {
        handler_visits: 0,
        string_bytes_copied: 0,
        rc_increments: 0,
        rc_decrements: 0,
        boxes: 0,
        unboxes: 0,
        peak_objects: 1,
    }
    ");
}

#[test]
fn fib() {
    insta::assert_debug_snapshot!(stats("fib", "75025\n"), @"
    RunStats {
        handler_visits: 0,
        string_bytes_copied: 5,
        rc_increments: 0,
        rc_decrements: 1,
        boxes: 0,
        unboxes: 0,
        peak_objects: 26,
    }
    ");
}

#[test]
fn list() {
    insta::assert_debug_snapshot!(stats("list", "10000100000\n"), @"
    RunStats {
        handler_visits: 0,
        string_bytes_copied: 11,
        rc_increments: 0,
        rc_decrements: 200001,
        boxes: 300001,
        unboxes: 300001,
        peak_objects: 100002,
    }
    ");
}

#[test]
fn loop_() {
    insta::assert_debug_snapshot!(stats("loop", "125000250000\n"), @"
    RunStats {
        handler_visits: 0,
        string_bytes_copied: 12,
        rc_increments: 0,
        rc_decrements: 1,
        boxes: 0,
        unboxes: 0,
        peak_objects: 2,
    }
    ");
}

#[test]
fn state() {
    insta::assert_debug_snapshot!(stats("state", "100000\n"), @"
    RunStats {
        handler_visits: 200000,
        string_bytes_copied: 6,
        rc_increments: 0,
        rc_decrements: 2,
        boxes: 100002,
        unboxes: 100001,
        peak_objects: 6,
    }
    ");
}

#[test]
fn tree() {
    insta::assert_debug_snapshot!(stats("tree", "14898 7454478318\n"), @"
    RunStats {
        handler_visits: 0,
        string_bytes_copied: 37,
        rc_increments: 14899,
        rc_decrements: 266465,
        boxes: 29797,
        unboxes: 266462,
        peak_objects: 14935,
    }
    ");
}
