//! 基準のプログラム (`bench/`) の実行の仕事の回数を、VM の検査モードで固定する
//! (docs/implementation/benchmarks.md の「回数のテスト」)。

use std::path::{Path, PathBuf};

use eml_test_support::run_stats;
use eml_vm::RunStats;

/// テストを持つ基準のプログラム。`bench/` にプログラムを足したら、ここと下のテストの関数の両方に足す。
const PROGRAMS: [&str; 18] = [
    "empty",
    "fib",
    "fold_state",
    "iter_generate",
    "iter_pipeline",
    "iter_pipeline_by_hand",
    "list",
    "loop",
    "orders",
    "orders_by_hand",
    "push_fold",
    "push_fold_by_hand",
    "state",
    "sum_loop",
    "tree",
    "tree_by_hand",
    "upsert",
    "upsert_by_hand",
];

fn bench_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench")
}

/// `debug_heap` 付きで走らせ、出力を確かめてから回数を返す。リークと実行時エラーは panic にする。
fn stats(name: &str, expected: &str) -> RunStats {
    let path = bench_dir().join(format!("{name}.em"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let (out, result) = run_stats(&text);
    // 実行時エラーやリークで止まると出力も欠けるので、出力の差だけでなく止まった理由も見せる
    assert_eq!(out, expected, "{result:?}");
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
    // 一覧に足しただけでテストの関数を書かないと、そのプログラムの回数が固定されないまま通る
    let source = include_str!("bench.rs");
    for name in PROGRAMS {
        assert!(
            source.contains(&format!("stats(\"{name}\"")),
            "`{name}` has no test function"
        );
    }
}

#[test]
fn empty() {
    insta::assert_debug_snapshot!(stats("empty", ""), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 0,
        rc_increments: 0,
        rc_decrements: 0,
        boxes: 0,
        unboxes: 0,
        allocations: 1,
        peak_objects: 1,
        peak_stack: 7,
    }
    ");
}

#[test]
fn fib() {
    insta::assert_debug_snapshot!(stats("fib", "75025\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 5,
        rc_increments: 0,
        rc_decrements: 1,
        boxes: 0,
        unboxes: 0,
        allocations: 2,
        peak_objects: 2,
        peak_stack: 132,
    }
    ");
}

#[test]
fn fold_state() {
    insta::assert_debug_snapshot!(stats("fold_state", "5000050000\n"), @"
    RunStats {
        evidence_steps: 100000,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 10,
        rc_increments: 400005,
        rc_decrements: 400017,
        boxes: 200003,
        unboxes: 200002,
        allocations: 13,
        peak_objects: 10,
        peak_stack: 45,
    }
    ");
}

#[test]
fn list() {
    insta::assert_debug_snapshot!(stats("list", "10000100000\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 11,
        rc_increments: 0,
        rc_decrements: 200001,
        boxes: 400000,
        unboxes: 400000,
        allocations: 200002,
        peak_objects: 100001,
        peak_stack: 700015,
    }
    ");
}

#[test]
fn loop_() {
    insta::assert_debug_snapshot!(stats("loop", "125000250000\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 12,
        rc_increments: 0,
        rc_decrements: 1,
        boxes: 0,
        unboxes: 0,
        allocations: 2,
        peak_objects: 2,
        peak_stack: 14,
    }
    ");
}

#[test]
fn state() {
    insta::assert_debug_snapshot!(stats("state", "100000\n"), @"
    RunStats {
        evidence_steps: 200000,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 6,
        rc_increments: 200001,
        rc_decrements: 200006,
        boxes: 100002,
        unboxes: 100001,
        allocations: 6,
        peak_objects: 4,
        peak_stack: 27,
    }
    ");
}

#[test]
fn tree() {
    insta::assert_debug_snapshot!(stats("tree", "14898 7454478318\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 37,
        rc_increments: 14898,
        rc_decrements: 266465,
        boxes: 44694,
        unboxes: 281359,
        allocations: 251568,
        peak_objects: 14900,
        peak_stack: 392,
    }
    ");
}

#[test]
fn iter_generate() {
    insta::assert_debug_snapshot!(stats("iter_generate", "5000050000\n"), @"
    RunStats {
        evidence_steps: 100000,
        yields: 100000,
        yield_frames: 100000,
        unwound_frames: 0,
        string_bytes_copied: 10,
        rc_increments: 300001,
        rc_decrements: 1300009,
        boxes: 500001,
        unboxes: 500001,
        allocations: 1000009,
        peak_objects: 10,
        peak_stack: 36,
    }
    ");
}

#[test]
fn iter_pipeline() {
    insta::assert_debug_snapshot!(stats("iter_pipeline", "166671666700000\n"), @"
    RunStats {
        evidence_steps: 100000,
        yields: 100000,
        yield_frames: 100000,
        unwound_frames: 0,
        string_bytes_copied: 15,
        rc_increments: 300001,
        rc_decrements: 1800013,
        boxes: 550001,
        unboxes: 600001,
        allocations: 1500013,
        peak_objects: 14,
        peak_stack: 52,
    }
    ");
}

#[test]
fn iter_pipeline_by_hand() {
    insta::assert_debug_snapshot!(stats("iter_pipeline_by_hand", "166671666700000\n"), @"
    RunStats {
        evidence_steps: 100000,
        yields: 100000,
        yield_frames: 100000,
        unwound_frames: 0,
        string_bytes_copied: 15,
        rc_increments: 300001,
        rc_decrements: 1800013,
        boxes: 350001,
        unboxes: 400001,
        allocations: 1500013,
        peak_objects: 14,
        peak_stack: 49,
    }
    ");
}

#[test]
fn orders() {
    insta::assert_debug_snapshot!(stats("orders", "150308561\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 9,
        rc_increments: 100000,
        rc_decrements: 459321,
        boxes: 219490,
        unboxes: 219490,
        allocations: 359322,
        peak_objects: 200001,
        peak_stack: 23,
    }
    ");
}

#[test]
fn orders_by_hand() {
    insta::assert_debug_snapshot!(stats("orders_by_hand", "150308561\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 9,
        rc_increments: 0,
        rc_decrements: 359321,
        boxes: 39830,
        unboxes: 39830,
        allocations: 359322,
        peak_objects: 200001,
        peak_stack: 17,
    }
    ");
}

#[test]
fn push_fold() {
    insta::assert_debug_snapshot!(stats("push_fold", "5000050000\n"), @"
    RunStats {
        evidence_steps: 100000,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 10,
        rc_increments: 300001,
        rc_decrements: 300008,
        boxes: 200002,
        unboxes: 200002,
        allocations: 8,
        peak_objects: 7,
        peak_stack: 37,
    }
    ");
}

#[test]
fn push_fold_by_hand() {
    insta::assert_debug_snapshot!(stats("push_fold_by_hand", "5000050000\n"), @"
    RunStats {
        evidence_steps: 100000,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 10,
        rc_increments: 100001,
        rc_decrements: 100007,
        boxes: 200002,
        unboxes: 200002,
        allocations: 7,
        peak_objects: 6,
        peak_stack: 29,
    }
    ");
}

#[test]
fn sum_loop() {
    insta::assert_debug_snapshot!(stats("sum_loop", "5000050000\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 10,
        rc_increments: 0,
        rc_decrements: 1,
        boxes: 0,
        unboxes: 0,
        allocations: 2,
        peak_objects: 2,
        peak_stack: 16,
    }
    ");
}

#[test]
fn tree_by_hand() {
    insta::assert_debug_snapshot!(stats("tree_by_hand", "14898 7454478318\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 37,
        rc_increments: 14898,
        rc_decrements: 266465,
        boxes: 14898,
        unboxes: 251563,
        allocations: 251568,
        peak_objects: 14900,
        peak_stack: 392,
    }
    ");
}

#[test]
fn upsert() {
    insta::assert_debug_snapshot!(stats("upsert", "2000 k1182 23\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 157580,
        rc_increments: 513768,
        rc_decrements: 892534,
        boxes: 39407,
        unboxes: 76222,
        allocations: 378767,
        peak_objects: 6002,
        peak_stack: 224,
    }
    ");
}

#[test]
fn upsert_by_hand() {
    insta::assert_debug_snapshot!(stats("upsert_by_hand", "2000 k1182 23\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 157580,
        rc_increments: 436140,
        rc_decrements: 814906,
        boxes: 20000,
        unboxes: 56815,
        allocations: 378767,
        peak_objects: 6002,
        peak_stack: 206,
    }
    ");
}

/// 確保の数は検査モードの `Stats::allocations` から来る。`list` は `range` と `map` が 100000 個ずつ `Cons` を作る。
#[test]
fn allocations_are_counted_in_checked_mode() {
    let stats = stats("list", "10000100000\n");
    assert!(stats.allocations >= 200_000, "{stats:?}");
}
