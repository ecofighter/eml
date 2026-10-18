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

/// 自動の版と手で特殊化した `_by_hand` の版の組と、両方の出力。
const PAIRS: [(&str, &str, &str); 5] = [
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
        reuses: 0,
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
        reuses: 0,
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
        reuses: 0,
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
        rc_decrements: 100001,
        boxes: 200000,
        unboxes: 200000,
        allocations: 100002,
        reuses: 100000,
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
        reuses: 0,
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
        reuses: 0,
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
        rc_decrements: 29800,
        boxes: 14898,
        unboxes: 251563,
        allocations: 14903,
        reuses: 236665,
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
        rc_decrements: 1,
        boxes: 0,
        unboxes: 0,
        allocations: 2,
        reuses: 0,
        peak_objects: 2,
        peak_stack: 15,
    }
    ");
}

/// S13e-3 の完了の条件 (docs/implementation/benchmarks.md の「S13e-3」)。`Iter.generate` を `Iter.fold` で引く形は、
/// 融合で1要素ごとの確保がなくなり、確保の数は入力の 10 万要素に比例しない。`Iter.filter` を重ねた形は、
/// 残した要素ごとに4つを確保する。`filter` が要素を飛ばすループは再帰する複製になり、作った `Some ((x, Iter k))`
/// を相手の分からない呼ぶ側へ返すためである。これをなくすには再帰する呼び出し先の継続についての特殊化が要り、後に回した
/// (ユーザーが決めた)。ここでは、S13e-3 の前の 900008 の約4分の1に減ったことを固定する。
#[test]
fn iter_allocations_do_not_grow_with_the_input() {
    let generate = stats("iter_generate", "5000050000\n");
    assert!(generate.allocations < 1000, "{generate:?}");
    for name in ["iter_pipeline", "iter_pipeline_by_hand"] {
        let stats = stats(name, "166671666700000\n");
        assert!(stats.allocations <= 200_100, "{name}: {stats:?}");
    }
}

/// S13e-2 の完了の条件は、特殊化した `Iter.generate` が1要素ごとに `k`、`Iter`、組、`Some` の4つを確保することだった。
/// S13e-3 の融合の後は、`Iter.fold` で引く形は1要素ごとに何も確保せず、確保の数は直接のループ `sum_loop` と同じ
/// になる。残りは結果を表示する分である (docs/implementation/benchmarks.md の「S13e-3」)。
#[test]
fn iter_generate_allocates_as_much_as_the_direct_loop() {
    let generate = stats("iter_generate", "5000050000\n");
    let direct = stats("sum_loop", "5000050000\n");
    assert_eq!(generate.allocations, direct.allocations, "{generate:?}");
    assert_eq!(generate.yields, 0, "{generate:?}");
}

/// S13e-2 の完了の条件 (docs/implementation/benchmarks.md の「S13e-2」)。`Iter.generate` で引く形は yield せず、push の
/// 形と状態の handler は evidence を引かない。
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
        rc_decrements: 200001,
        boxes: 150001,
        unboxes: 250000,
        allocations: 200002,
        reuses: 0,
        peak_objects: 5,
        peak_stack: 26,
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
        rc_decrements: 200001,
        boxes: 150001,
        unboxes: 250000,
        allocations: 200002,
        reuses: 0,
        peak_objects: 5,
        peak_stack: 26,
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
        rc_decrements: 200001,
        boxes: 39830,
        unboxes: 39830,
        allocations: 200002,
        reuses: 159320,
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
        rc_decrements: 200001,
        boxes: 39830,
        unboxes: 39830,
        allocations: 200002,
        reuses: 159320,
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
        reuses: 0,
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
        reuses: 0,
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
        reuses: 0,
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
        rc_decrements: 29800,
        boxes: 14898,
        unboxes: 251563,
        allocations: 14903,
        reuses: 236665,
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
        rc_decrements: 484142,
        boxes: 20000,
        unboxes: 56815,
        allocations: 48003,
        reuses: 312764,
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
        rc_decrements: 484142,
        boxes: 20000,
        unboxes: 56815,
        allocations: 48003,
        reuses: 312764,
        peak_objects: 6002,
        peak_stack: 352,
    }
    ");
}

/// 確保の数は検査モードの `Stats::allocations` から来る。`list` は `range` が 100000 個の `Cons` を作り、`map` はその箱を
/// 使い回す (docs/spec/lir.md の「reuse」)。
#[test]
fn allocations_are_counted_in_checked_mode() {
    let stats = stats("list", "10000100000\n");
    assert!(stats.allocations >= 100_000, "{stats:?}");
}

/// S13e-1 の完了の条件 (docs/implementation/benchmarks.md の「S13e-1」)。既知の閉包を渡す版と手で特殊化した版で、
/// 確保と `box` / `unbox` の回数が同じになる。RC の回数は、本体を埋め込む S13e-3 で揃うので、下の
/// `specialization_matches_hand_written_rc` が比べる。
#[test]
fn specialization_matches_hand_written_work() {
    for (name, by_hand, output) in PAIRS {
        let written = stats(name, output);
        let hand = stats(by_hand, output);
        assert_eq!(
            (written.allocations, written.boxes, written.unboxes),
            (hand.allocations, hand.boxes, hand.unboxes),
            "{name} against {by_hand}: (allocations, boxes, unboxes)"
        );
    }
}

/// S13e-3 の完了の条件 (docs/implementation/benchmarks.md の「S13e-3」)。インライン化と融合の後は、自動の版と
/// `_by_hand` の版の5つの組で、RC の回数も同じになる。
#[test]
fn specialization_matches_hand_written_rc() {
    for (name, by_hand, output) in PAIRS {
        let written = stats(name, output);
        let hand = stats(by_hand, output);
        assert_eq!(
            (written.rc_increments, written.rc_decrements),
            (hand.rc_increments, hand.rc_decrements),
            "{name} against {by_hand}: (rc_increments, rc_decrements)"
        );
    }
}
