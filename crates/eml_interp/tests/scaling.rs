//! 実行の仕事の回数 (`RunStats`) を比べるテスト。時間ではなく回数を比べるので、`#[ignore]` を付けずにふだんの
//! `cargo test` で流す。ソースはテストの中で作る。

use eml_interp::RunStats;
use eml_test_support::run_stats;

/// 正常に終わった実行の回数。生成したソースが意図どおりに走ったことを、出力でも確かめる。
fn stats(text: &str, expected: &str) -> RunStats {
    let (out, result) = run_stats(text);
    assert_eq!(out, expected);
    result.unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn a_program_without_operations_or_strings_does_no_counted_work() {
    let stats = stats("main : Unit -> <IO> Unit\nmain () = ()", "");
    assert_eq!(stats, RunStats::default());
}

/// どの `perform` も、少なくとも見つけた handler のフレームを調べる。
#[test]
fn every_operation_visits_at_least_its_handler() {
    let text = [
        "effect Ask where",
        "  ask : Unit -> Int",
        "",
        "main : Unit -> <IO> Unit",
        "main () =",
        "  let n = handle ask () + ask () + ask () with",
        "            | ask () k -> k 1",
        "  println (show_int n)",
    ]
    .join("\n");
    let stats = stats(&text, "3\n");
    assert!(stats.handler_visits >= 3, "{stats:?}");
}

/// 連結の結果の文字列には、両辺の中身を書く。
#[test]
fn concatenation_writes_the_bytes_of_both_sides() {
    let stats = stats(
        "main : Unit -> <IO> Unit\nmain () = println (\"ab\" ++ \"cd\")",
        "abcd\n",
    );
    assert!(stats.string_bytes_copied >= 4, "{stats:?}");
    assert_eq!(stats.handler_visits, 0);
}
