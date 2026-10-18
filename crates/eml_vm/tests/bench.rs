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
        boxes: 200000,
        unboxes: 200000,
        allocations: 200002,
        peak_objects: 100001,
        peak_stack: 600014,
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
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 6,
        rc_increments: 0,
        rc_decrements: 2,
        boxes: 2,
        unboxes: 1,
        allocations: 3,
        peak_objects: 2,
        peak_stack: 16,
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
        boxes: 14898,
        unboxes: 251563,
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
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 10,
        rc_increments: 0,
        rc_decrements: 400003,
        boxes: 200001,
        unboxes: 300001,
        allocations: 400004,
        peak_objects: 5,
        peak_stack: 22,
    }
    ");
}

/// S13e-2 の完了の条件。特殊化した `Iter.generate` は、1要素ごとに `k`、`Iter`、組、`Some` の4つを確保する。残りは
/// 前後の準備の分である
/// (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「`Iter` と push の形での結果 (見込み)」)。
#[test]
fn iter_generate_allocates_four_objects_per_element() {
    let stats = stats("iter_generate", "5000050000\n");
    assert!(stats.allocations <= 100_000 * 4 + 100, "{stats:?}");
    assert_eq!(stats.yields, 0, "{stats:?}");
}

/// S13e-2 の完了の条件 (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「作業の分け方と完了の
/// 条件」)。`Iter.generate` で引く形は yield せず、push の形と状態の handler は evidence を引かない。
#[test]
fn handler_specialization_removes_yields_and_evidence() {
    let generate = stats("iter_generate", "5000050000\n");
    assert_eq!(generate.yields, 0, "{generate:?}");
    let push = stats("push_fold", "5000050000\n");
    assert_eq!(push.evidence_steps, 0, "{push:?}");
    let fold = stats("fold_state", "5000050000\n");
    assert_eq!(fold.evidence_steps, 0, "{fold:?}");
    let pipeline = stats("iter_pipeline", "166671666700000\n");
    assert_eq!(pipeline.yields, 0, "{pipeline:?}");
}

#[test]
fn iter_pipeline() {
    insta::assert_debug_snapshot!(stats("iter_pipeline", "166671666700000\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 15,
        rc_increments: 0,
        rc_decrements: 800007,
        boxes: 250001,
        unboxes: 400001,
        allocations: 800008,
        peak_objects: 9,
        peak_stack: 37,
    }
    ");
}

#[test]
fn iter_pipeline_by_hand() {
    insta::assert_debug_snapshot!(stats("iter_pipeline_by_hand", "166671666700000\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 15,
        rc_increments: 0,
        rc_decrements: 800007,
        boxes: 250001,
        unboxes: 400001,
        allocations: 800008,
        peak_objects: 9,
        peak_stack: 37,
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
        rc_increments: 0,
        rc_decrements: 359321,
        boxes: 39830,
        unboxes: 39830,
        allocations: 359322,
        peak_objects: 200001,
        peak_stack: 20,
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
        peak_stack: 19,
    }
    ");
}

#[test]
fn push_fold() {
    insta::assert_debug_snapshot!(stats("push_fold", "5000050000\n"), @"
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
fn push_fold_by_hand() {
    insta::assert_debug_snapshot!(stats("push_fold_by_hand", "5000050000\n"), @"
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
        rc_increments: 436140,
        rc_decrements: 796906,
        boxes: 20000,
        unboxes: 243909,
        allocations: 360767,
        peak_objects: 6002,
        peak_stack: 352,
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
        rc_decrements: 796906,
        boxes: 20000,
        unboxes: 243909,
        allocations: 360767,
        peak_objects: 6002,
        peak_stack: 352,
    }
    ");
}

/// 確保の数は検査モードの `Stats::allocations` から来る。`list` は `range` と `map` が 100000 個ずつ `Cons` を作る。
#[test]
fn allocations_are_counted_in_checked_mode() {
    let stats = stats("list", "10000100000\n");
    assert!(stats.allocations >= 200_000, "{stats:?}");
}

/// S13e-1 の完了の条件 (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「作業の分け方と完了の条件」)。
/// 既知の閉包を渡す版と手で特殊化した版で、確保と `box` / `unbox` の回数が同じになる。RC の回数は、本体を埋め込む
/// S13e-3 で揃えるので、ここでは比べない。
#[test]
fn specialization_matches_hand_written_work() {
    let pairs = [
        ("orders", "orders_by_hand", "150308561\n"),
        ("push_fold", "push_fold_by_hand", "5000050000\n"),
        (
            "iter_pipeline",
            "iter_pipeline_by_hand",
            "166671666700000\n",
        ),
        ("tree", "tree_by_hand", "14898 7454478318\n"),
        ("upsert", "upsert_by_hand", "2000 k1182 23\n"),
    ];
    for (name, by_hand, output) in pairs {
        let written = stats(name, output);
        let hand = stats(by_hand, output);
        assert_eq!(
            (written.allocations, written.boxes, written.unboxes),
            (hand.allocations, hand.boxes, hand.unboxes),
            "{name} against {by_hand}: (allocations, boxes, unboxes)"
        );
    }
}
