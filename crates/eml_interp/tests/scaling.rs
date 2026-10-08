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

#[test]
fn evaluating_a_literal_copies_nothing() {
    // 文字列のリテラルは不死の物体で、`Rhs::ConstString` は中身を写さずに参照を1つ作る (docs/spec/runtime.md)。
    // 64 バイトのリテラルを n 回評価して文字列の `Switch` で比べるので、1回でも写すと上限を超える。残る数は
    // `show_int` の結果の分だけである
    let literal = "x".repeat(64);
    let n = 2000;
    let source = format!(
        "same : String -> Int\n\
         same s = match s with\n  | \"{literal}\" -> 1\n  | _ -> 0\n\n\
         count : Int -> Int -> Int\n\
         count n acc = if n == 0 then acc else count (n - 1) (acc + same \"{literal}\")\n\n\
         main : Unit -> <IO> Unit\n\
         main () = println (show_int (count {n} 0))\n"
    );
    let (out, stats) = eml_test_support::run_stats(&source);
    assert_eq!(out, format!("{n}\n"));
    let stats = stats.unwrap();
    assert!(
        stats.string_bytes_copied < literal.len() as u64,
        "{stats:?}"
    );
}

/// リテラルから始めて、一意な左辺に `"x"` を n 回つなぐプログラム。`grow` の `acc` は各枝で1回だけ使うので、
/// Perceus は `dup` を入れず、2回目からの `++` には RC が 1 の左辺が届く。
fn growing_string(n: usize) -> String {
    format!(
        "grow : Int -> String -> String\n\
         grow n acc = if n == 0 then acc else grow (n - 1) (acc ++ \"x\")\n\
         \n\
         main : Unit -> <IO> Unit\n\
         main () = println (grow {n} \"s\")\n"
    )
}

#[test]
fn appending_to_a_unique_string_copies_only_the_right_side() {
    // 最初の `++` だけが不死のリテラル "s" を写し、残りは一意な左辺の後に 1 バイトずつ足す。左辺を毎回写すと、
    // 2 + 3 + … + (n + 1) バイトになる
    let n = 2000;
    let (out, result) = eml_test_support::run_stats(&growing_string(n));
    let stats = result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, format!("s{}\n", "x".repeat(n)));
    assert!(stats.string_bytes_copied <= 4 * n as u64, "{stats:?}");
}
