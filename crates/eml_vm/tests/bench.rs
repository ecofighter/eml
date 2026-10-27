//! 基準のプログラム (`bench/`) の実行の仕事の回数を、VM の検査モードで固定する
//! (docs/implementation/benchmarks.md の「回数のテスト」)。

use std::path::{Path, PathBuf};

use eml_test_support::{lir, lir_files, run_stats, run_stats_with_args};
use eml_vm::RunStats;

/// テストを持つ基準のプログラム。`bench/` にプログラムを足したら、ここと下のテストの関数の両方に足す。
const PROGRAMS: [&str; 40] = [
    "apply_float",
    "apply_int",
    "byte_loop",
    "bytes_loop",
    "char_loop",
    "empty",
    "ep_float",
    "fannkuch_by_hand",
    "fannkuch_var",
    "fib",
    "fold_state",
    "iter_generate",
    "iter_pipeline",
    "iter_pipeline_by_hand",
    "list",
    "loop",
    "mandelbrot",
    "nbody",
    "orders",
    "orders_by_hand",
    "push_fold",
    "push_fold_by_hand",
    "rbtree",
    "records",
    "records_wide",
    "sieve_by_hand",
    "sieve_var",
    "spectral_norm",
    "state",
    "state_loop",
    "state_loop_by_hand",
    "state_nested",
    "std_list",
    "std_map",
    "sum_loop",
    "tree",
    "tree_by_hand",
    "tree_map",
    "upsert",
    "upsert_by_hand",
];

/// 自動の版と手で特殊化した `_by_hand` の版の組と、両方の出力。`state_loop` の組は、状態を渡す形の
/// 特殊化と、状態を手で渡す版を比べる (docs/implementation/benchmarks.md の「回数のテスト」)。
const PAIRS: [(&str, &str, &str); 6] = [
    ("orders", "orders_by_hand", "150308561\n"),
    ("push_fold", "push_fold_by_hand", "5000050000\n"),
    (
        "iter_pipeline",
        "iter_pipeline_by_hand",
        "166671666700000\n",
    ),
    ("tree", "tree_by_hand", "14898 7454478318\n"),
    ("upsert", "upsert_by_hand", "2000 k1182 23\n"),
    ("state_loop", "state_loop_by_hand", "5000050000\n"),
];

fn bench_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench")
}

/// `debug_heap` 付きで走らせ、出力を確かめてから回数を返す。リークと実行時エラーは panic にする。
fn stats(name: &str, expected: &str) -> RunStats {
    stats_at(&bench_dir().join(format!("{name}.em")), expected)
}

/// `stats` と同じく流し、プログラムに引数 `args` (大きさ) を渡す。
fn stats_with_args(name: &str, args: &[&str], expected: &str) -> RunStats {
    let path = bench_dir().join(format!("{name}.em"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let (out, result) = run_stats_with_args(&text, args);
    assert_eq!(out, expected, "{result:?}");
    result.unwrap_or_else(|error| panic!("{error}"))
}

/// `stats` と同じく、`tests/ui/run/` の下の UI テストのプログラムを走らせる。`path` はそこからの相対パスである。
fn ui_stats(path: &str, expected: &str) -> RunStats {
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/ui/run");
    stats_at(&ui.join(path), expected)
}

fn stats_at(path: &Path, expected: &str) -> RunStats {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
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
fn byte_loop() {
    insta::assert_debug_snapshot!(stats("byte_loop", "11999995\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 100008,
        rc_increments: 0,
        rc_decrements: 16,
        boxes: 0,
        unboxes: 0,
        allocations: 17,
        reuses: 0,
        peak_objects: 3,
        peak_stack: 17,
    }
    ");
}

#[test]
fn char_loop() {
    insta::assert_debug_snapshot!(stats("char_loop", "11999995\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 100008,
        rc_increments: 0,
        rc_decrements: 16,
        boxes: 0,
        unboxes: 0,
        allocations: 17,
        reuses: 0,
        peak_objects: 3,
        peak_stack: 19,
    }
    ");
}

#[test]
fn bytes_loop() {
    insta::assert_debug_snapshot!(stats("bytes_loop", "6500000\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 100007,
        rc_increments: 0,
        rc_decrements: 17,
        boxes: 0,
        unboxes: 0,
        allocations: 18,
        reuses: 0,
        peak_objects: 3,
        peak_stack: 17,
    }
    ");
}

#[test]
fn apply_float() {
    insta::assert_debug_snapshot!(stats("apply_float", "2.0\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 3,
        rc_increments: 0,
        rc_decrements: 200001,
        boxes: 200000,
        unboxes: 200000,
        allocations: 200002,
        reuses: 0,
        peak_objects: 2,
        peak_stack: 23,
    }
    ");
}

#[test]
fn apply_int() {
    insta::assert_debug_snapshot!(stats("apply_int", "100000\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 6,
        rc_increments: 0,
        rc_decrements: 1,
        boxes: 200000,
        unboxes: 200000,
        allocations: 2,
        reuses: 0,
        peak_objects: 2,
        peak_stack: 23,
    }
    ");
}

#[test]
fn ep_float() {
    insta::assert_debug_snapshot!(stats("ep_float", "2500025000.0\n"), @"
    RunStats {
        evidence_steps: 200000,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 12,
        rc_increments: 400004,
        rc_decrements: 400012,
        boxes: 100006,
        unboxes: 100004,
        allocations: 9,
        reuses: 0,
        peak_objects: 8,
        peak_stack: 44,
    }
    ");
}

#[test]
fn records_wide() {
    insta::assert_debug_snapshot!(
        stats("records_wide", "100000 49764648 999 223 33086607712 50000 678\n"),
        @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 78,
        rc_increments: 0,
        rc_decrements: 11,
        boxes: 0,
        unboxes: 0,
        allocations: 12,
        reuses: 100000,
        peak_objects: 5,
        peak_stack: 41,
    }
    "
    );
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

#[test]
fn state_loop() {
    insta::assert_debug_snapshot!(stats("state_loop", "5000050000\n"), @"
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
fn state_loop_by_hand() {
    insta::assert_debug_snapshot!(stats("state_loop_by_hand", "5000050000\n"), @"
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
fn state_nested() {
    insta::assert_debug_snapshot!(stats("state_nested", "5000050000 100000\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 23,
        rc_increments: 0,
        rc_decrements: 3,
        boxes: 0,
        unboxes: 0,
        allocations: 4,
        reuses: 0,
        peak_objects: 4,
        peak_stack: 20,
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

/// 融合の確かめ (docs/implementation/benchmarks.md の「回数のテスト」)。`Iter.generate` を `Iter.fold` で引く形は、
/// 融合で1要素ごとの確保がなくなり、確保の数は入力の 10 万要素に比例しない。`Iter.filter` を重ねた形は、
/// 残した要素ごとに4つを確保する。`filter` が要素を飛ばすループは再帰する複製になり、作った `Some ((x, Iter k))`
/// を相手の分からない呼ぶ側へ返すためである。これをなくすには再帰する呼び出し先の継続についての特殊化が要り、後に回した
/// (ユーザーが決めた)。ここでは、残した要素ごとの4つの確保の上限 (200,100) を固定する。
#[test]
fn iter_allocations_do_not_grow_with_the_input() {
    let generate = stats("iter_generate", "5000050000\n");
    assert!(generate.allocations < 1000, "{generate:?}");
    for name in ["iter_pipeline", "iter_pipeline_by_hand"] {
        let stats = stats(name, "166671666700000\n");
        assert!(stats.allocations <= 200_100, "{name}: {stats:?}");
    }
}

/// 融合の後は、`Iter.fold` で引く形は1要素ごとに何も確保せず、確保の数は直接のループ `sum_loop` と同じ
/// になる。残りは結果を表示する分である (docs/implementation/benchmarks.md の「回数のテスト」)。
#[test]
fn iter_generate_allocates_as_much_as_the_direct_loop() {
    let generate = stats("iter_generate", "5000050000\n");
    let direct = stats("sum_loop", "5000050000\n");
    assert_eq!(generate.allocations, direct.allocations, "{generate:?}");
    assert_eq!(generate.yields, 0, "{generate:?}");
}

/// handler の特殊化の確かめ (docs/implementation/benchmarks.md の「回数のテスト」)。`Iter.generate` で引く形は yield せず、push の
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
    // 状態を渡す形への特殊化の確かめ (docs/implementation/benchmarks.md の「回数のテスト」)。末尾でない `put` のループと、状態つきの
    // handler を2つ重ねたループも、状態を渡す形に特殊化されて evidence を引かない
    let state_loop = stats("state_loop", "5000050000\n");
    assert_eq!(state_loop.evidence_steps, 0, "{state_loop:?}");
    let nested = stats("state_nested", "5000050000 100000\n");
    assert_eq!(nested.evidence_steps, 0, "{nested:?}");
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
        boxes: 50000,
        unboxes: 50000,
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
        boxes: 50000,
        unboxes: 50000,
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
        peak_stack: 18,
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
        peak_stack: 18,
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
        rc_increments: 25952,
        rc_decrements: 71955,
        boxes: 22000,
        unboxes: 58815,
        allocations: 46004,
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
        rc_increments: 25952,
        rc_decrements: 71955,
        boxes: 22000,
        unboxes: 58815,
        allocations: 46004,
        reuses: 312764,
        peak_objects: 6002,
        peak_stack: 352,
    }
    ");
}

#[test]
fn records() {
    insta::assert_debug_snapshot!(stats("records", "100000 49764648 999 223\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 37,
        rc_increments: 0,
        rc_decrements: 6,
        boxes: 0,
        unboxes: 0,
        allocations: 7,
        reuses: 0,
        peak_objects: 4,
        peak_stack: 26,
    }
    ");
}

#[test]
fn rbtree() {
    insta::assert_debug_snapshot!(stats("rbtree", "9997\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 4,
        rc_increments: 0,
        rc_decrements: 24497,
        boxes: 0,
        unboxes: 0,
        allocations: 24498,
        reuses: 128498,
        peak_objects: 9998,
        peak_stack: 259,
    }
    ");
}

#[test]
fn tree_map() {
    insta::assert_debug_snapshot!(stats("tree_map", "33714156\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 8,
        rc_increments: 0,
        rc_decrements: 8192,
        boxes: 0,
        unboxes: 0,
        allocations: 8193,
        reuses: 163820,
        peak_objects: 8192,
        peak_stack: 126,
    }
    ");
}

#[test]
fn std_list() {
    insta::assert_debug_snapshot!(stats("std_list", "887099805\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 9,
        rc_increments: 0,
        rc_decrements: 20001,
        boxes: 91430,
        unboxes: 148574,
        allocations: 20002,
        reuses: 257148,
        peak_objects: 20001,
        peak_stack: 23,
    }
    ");
}

#[test]
fn std_map() {
    insta::assert_debug_snapshot!(stats("std_map", "2000 k1182 23\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 157580,
        rc_increments: 2000,
        rc_decrements: 50003,
        boxes: 22000,
        unboxes: 58777,
        allocations: 48004,
        reuses: 250381,
        peak_objects: 8001,
        peak_stack: 274,
    }
    ");
}

#[test]
fn mandelbrot() {
    insta::assert_debug_snapshot!(stats("mandelbrot", "6518\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 4,
        rc_increments: 0,
        rc_decrements: 1,
        boxes: 0,
        unboxes: 0,
        allocations: 2,
        reuses: 0,
        peak_objects: 2,
        peak_stack: 39,
    }
    ");
}

#[test]
fn nbody() {
    insta::assert_debug_snapshot!(stats("nbody", "-0.169075164\n-0.169087605\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 24,
        rc_increments: 82160,
        rc_decrements: 107210,
        boxes: 5,
        unboxes: 20055,
        allocations: 25051,
        reuses: 0,
        peak_objects: 26,
        peak_stack: 76,
    }
    ");
}

#[test]
fn spectral_norm() {
    insta::assert_debug_snapshot!(stats("spectral_norm", "1.274219991\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 11,
        rc_increments: 398371,
        rc_decrements: 402413,
        boxes: 4001,
        unboxes: 400400,
        allocations: 4043,
        reuses: 0,
        peak_objects: 304,
        peak_stack: 51,
    }
    ");
}

#[test]
fn sieve_var() {
    insta::assert_debug_snapshot!(stats("sieve_var", "2262\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 4,
        rc_increments: 0,
        rc_decrements: 2,
        boxes: 0,
        unboxes: 39996,
        allocations: 3,
        reuses: 0,
        peak_objects: 2,
        peak_stack: 26,
    }
    ");
}

#[test]
fn sieve_by_hand() {
    insta::assert_debug_snapshot!(stats("sieve_by_hand", "2262\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 4,
        rc_increments: 0,
        rc_decrements: 2,
        boxes: 0,
        unboxes: 39996,
        allocations: 3,
        reuses: 0,
        peak_objects: 2,
        peak_stack: 25,
    }
    ");
}

#[test]
fn fannkuch_var() {
    insta::assert_debug_snapshot!(stats("fannkuch_var", "228\nPfannkuchen(7) = 16\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 25,
        rc_increments: 0,
        rc_decrements: 9,
        boxes: 12293,
        unboxes: 40868,
        allocations: 10,
        reuses: 0,
        peak_objects: 5,
        peak_stack: 54,
    }
    ");
}

#[test]
fn fannkuch_by_hand() {
    insta::assert_debug_snapshot!(stats("fannkuch_by_hand", "228\nPfannkuchen(7) = 16\n"), @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 25,
        rc_increments: 0,
        rc_decrements: 8,
        boxes: 20947,
        unboxes: 40866,
        allocations: 9,
        reuses: 0,
        peak_objects: 4,
        peak_stack: 21,
    }
    ");
}

/// `var` の確かめ (docs/implementation/benchmarks.md の「回数のテスト」)。`var` で書いた版と、`MutArray` を線形に
/// 受け渡して手で書いた版の組と、両方の出力。
const VAR_PAIRS: [(&str, &str, &str); 2] = [
    ("sieve_var", "sieve_by_hand", "2262\n"),
    (
        "fannkuch_var",
        "fannkuch_by_hand",
        "228\nPfannkuchen(7) = 16\n",
    ),
];

/// `var` の版が手書きの版より多く確保する数。経路の関数の末尾でない呼び出しから2つ以上の値が戻る所のタプルは、
/// 構造体になり、確保しなくなった。fannkuch-redux に残る 1 は、`main` が受ける 8 つの値の組で、6 語を
/// 超えるので構造体にしない (docs/spec/core-ir.md の「構造体にする型」)。
const RESIDUAL: [(&str, u64); 2] = [("sieve_var", 0), ("fannkuch_var", 1)];

#[test]
fn var_matches_hand_written_work() {
    for (name, by_hand, output) in VAR_PAIRS {
        let written = stats(name, output);
        let hand = stats(by_hand, output);
        assert_eq!(written.evidence_steps, 0, "{name}: {written:?}");
        assert_eq!(written.yields, 0, "{name}: {written:?}");
        let residual = RESIDUAL
            .iter()
            .find(|(program, _)| *program == name)
            .map(|(_, residual)| *residual)
            .expect("a residual for every pair");
        assert_eq!(
            written.allocations,
            hand.allocations + residual,
            "{name} against {by_hand}: allocations"
        );
    }
}

/// reuse の確かめ (docs/implementation/benchmarks.md の「回数のテスト」)。関数の中で手放して作り直す8つのプログラムで、
/// `con` が手放した箱を使い回す (docs/spec/lir.md の「reuse」)。
#[test]
fn rebuilding_programs_reuse_boxes() {
    for (name, output) in [
        ("list", "10000100000\n"),
        ("tree", "14898 7454478318\n"),
        ("upsert", "2000 k1182 23\n"),
        ("orders", "150308561\n"),
        (
            "records_wide",
            "100000 49764648 999 223 33086607712 50000 678\n",
        ),
        ("rbtree", "9997\n"),
        ("tree_map", "33714156\n"),
        ("std_list", "887099805\n"),
        ("std_map", "2000 k1182 23\n"),
    ] {
        let stats = stats(name, output);
        assert!(stats.reuses > 0, "{name}: {stats:?}");
    }
}

/// 確保の数は検査モードの `Stats::allocations` から来る。`list` は `range` が 100000 個の `Cons` を作り、`map` はその箱を
/// 使い回す (docs/spec/lir.md の「reuse」)。
#[test]
fn allocations_are_counted_in_checked_mode() {
    let stats = stats("list", "10000100000\n");
    assert!(stats.allocations >= 100_000, "{stats:?}");
}

/// 既知の閉包の確かめ (docs/implementation/benchmarks.md の「回数のテスト」)。既知の閉包を渡す版と手で特殊化した版で、
/// 確保と `box` / `unbox` の回数が同じになる。RC の回数は、本体を埋め込む融合で揃うので、下の
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

/// インライン化と融合の確かめ (docs/implementation/benchmarks.md の「回数のテスト」)。インライン化と融合の後は、自動の版と
/// `_by_hand` の版の6つの組で、RC の回数も同じになる。
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

/// 巻き戻しの地点の総数。値に依存する実行時エラーを `Panic` に移すと、地点の数が動く。
fn unwind_point_count(program: &eml_lir::Program) -> usize {
    eml_lir::unwind::points_all(program)
        .iter()
        .filter_map(|points| points.as_ref().ok())
        .map(Vec::len)
        .sum()
}

#[test]
fn unwind_points() {
    let scripts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/ui/run/scripts");
    let mut lines = Vec::new();
    for name in PROGRAMS {
        let text = std::fs::read_to_string(bench_dir().join(format!("{name}.em"))).unwrap();
        lines.push(format!("{name}: {}", unwind_point_count(&lir(&text))));
    }
    for name in ["wc", "grep", "logstat", "csv2jsonl", "deploy"] {
        let text = std::fs::read_to_string(scripts.join(name).join("main.em")).unwrap();
        lines.push(format!(
            "scripts/{name}: {}",
            unwind_point_count(&lir_files(&text, &[]))
        ));
    }
    insta::assert_snapshot!(lines.join("\n"), @"
    apply_float: 9
    apply_int: 13
    byte_loop: 7
    bytes_loop: 7
    char_loop: 8
    empty: 0
    ep_float: 15
    fannkuch_by_hand: 39
    fannkuch_var: 83
    fib: 7
    fold_state: 4
    iter_generate: 5
    iter_pipeline: 11
    iter_pipeline_by_hand: 11
    list: 9
    loop: 4
    mandelbrot: 29
    nbody: 93
    orders: 12
    orders_by_hand: 12
    push_fold: 4
    push_fold_by_hand: 4
    rbtree: 12
    records: 9
    records_wide: 13
    sieve_by_hand: 14
    sieve_var: 36
    spectral_norm: 73
    state: 4
    state_loop: 4
    state_loop_by_hand: 4
    state_nested: 5
    std_list: 13
    std_map: 20
    sum_loop: 4
    tree: 15
    tree_by_hand: 15
    tree_map: 20
    upsert: 23
    upsert_by_hand: 23
    scripts/wc: 46
    scripts/grep: 61
    scripts/logstat: 106
    scripts/csv2jsonl: 112
    scripts/deploy: 110
    ");
}

/// 呼び出しの規約と構造体の表現の引き換え (docs/implementation/benchmarks.md の「呼び出しの規約の引き換え」と「構造体の表現の引き換え」)。特殊化されない `List.map` が呼ぶ関数の値は結果が一様なので、
/// 受けた `Float` をそのまま返すラムダの版が、要素ごとに結果の `Float` の箱を1つ作る。組を入れ替える関数の値を渡すと、ラムダが要素の組を箱から
/// 取り出して入れ替え、新しい箱に入れて返す。その回数を固定して記録する。特殊化される組の入れ替えと、RC の葉を3つ持つ組を展開しない関数に通す形も
/// 固定する。
#[test]
fn trade_offs() {
    insta::assert_debug_snapshot!(
        ui_stats("functions/unspecialized_map_passes_floats.em", "49.5\n"),
        @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 4,
        rc_increments: 0,
        rc_decrements: 21,
        boxes: 20,
        unboxes: 20,
        allocations: 22,
        reuses: 20,
        peak_objects: 21,
        peak_stack: 27,
    }
    "
    );
    insta::assert_debug_snapshot!(
        ui_stats("records/swap_map_unspecialized.em", "192.5\n"),
        @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 5,
        rc_increments: 0,
        rc_decrements: 41,
        boxes: 50,
        unboxes: 50,
        allocations: 42,
        reuses: 50,
        peak_objects: 31,
        peak_stack: 28,
    }
    "
    );
    let specialized = ui_stats("records/swap_map_specialized.em", "192.5\n");
    assert_eq!(
        (specialized.allocations, specialized.reuses),
        SWAP_MAP_BEFORE_STRUCTS,
        "{specialized:?}"
    );
    insta::assert_debug_snapshot!(specialized, @"
    RunStats {
        evidence_steps: 0,
        yields: 0,
        yield_frames: 0,
        unwound_frames: 0,
        string_bytes_copied: 5,
        rc_increments: 0,
        rc_decrements: 31,
        boxes: 30,
        unboxes: 30,
        allocations: 32,
        reuses: 50,
        peak_objects: 31,
        peak_stack: 18,
    }
    ");
    insta::assert_debug_snapshot!(
        ui_stats(
            "records/three_rc_leaves_through_a_call.em",
            "123459\n9,12,345\n"
        )
        .rc_increments,
        @"3"
    );
}

/// 構造体の確かめ (docs/implementation/benchmarks.md の「回数のテスト」)。nbody を1回
/// 進めるごとの確保は、配列に書く `Vec3` の箱の 25 (速度に 20、位置に 5) である。計算の途中の `Vec3` と、状態を渡す形の
/// 複製が返す `(MutArray, MutArray)` の組は確保しない。
#[test]
fn nbody_allocates_25_per_step() {
    let one = stats_with_args("nbody", &["1"], "-0.169075164\n-0.169074954\n");
    let thousand = stats_with_args("nbody", &["1000"], "-0.169075164\n-0.169087605\n");
    let two_thousand = stats_with_args("nbody", &["2000"], "-0.169075164\n-0.169071607\n");
    assert_eq!(
        thousand.allocations - one.allocations,
        999 * 25,
        "{one:?}\n{thousand:?}"
    );
    assert_eq!(
        two_thousand.allocations - thousand.allocations,
        1000 * 25,
        "{thousand:?}\n{two_thousand:?}"
    );
}

/// mandelbrot の `Complex` は構造体なので、格子点ごとの確保も、まわすたびの reuse もない。確保は
/// 格子の大きさによらない定数だけである。
#[test]
fn mandelbrot_does_not_allocate_complex() {
    let small = stats_with_args("mandelbrot", &["64"], "1626\n");
    let large = stats_with_args("mandelbrot", &["128"], "6518\n");
    assert_eq!(large.reuses, 0, "{large:?}");
    assert_eq!(small.reuses, 0, "{small:?}");
    assert_eq!(small.allocations, large.allocations, "{small:?}\n{large:?}");
}

/// 組を構造体にしない場合の spectral-norm の確保の数 (docs/implementation/benchmarks.md の「配列の `Float` の要素の箱」)。
const SPECTRAL_NORM_BOXED_TUPLE_ALLOCATIONS: u64 = 4057;

/// 状態を渡す形の複製が返す組の 12 と、その中の `Float` の箱の 2 の、14 の確保が減る。配列の
/// `Float` の要素の箱は残る。
#[test]
fn spectral_norm_allocates_14_fewer() {
    let stats = stats("spectral_norm", "1.274219991\n");
    assert_eq!(
        stats.allocations,
        SPECTRAL_NORM_BOXED_TUPLE_ALLOCATIONS - 14,
        "{stats:?}"
    );
}

/// 組の要素を入れ替える特殊化された `map` (`records/swap_map_specialized.em`) の、構造体の表現の前
/// の `allocations` と `reuses`。葉の単位の覗き穴と reuse で、構造体の後も同じ数になる
/// (docs/implementation/benchmarks.md の「構造体の表現の引き換え」)。
const SWAP_MAP_BEFORE_STRUCTS: (u64, u64) = (32, 50);
