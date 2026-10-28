//! `Std.IoTest` の操作の記録と driver の完了 (docs/spec/vm.md の「`Async` のタスク」)。テストの driver
//! (`Clock::Virtual`) の上で流す。

use eml_test_support::{run, run_stats};

/// 書きは driver の操作を1つ出し、完了を1つ受けて、待っていたタスクを再開する。待ちは継続を1つ渡す。
#[test]
fn a_write_counts_one_completion() {
    let src = "\
import Async
import Bytes
import IoTest
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = IoTest.pair ()
  let (n, a) = IoTest.write (Bytes.of_string \"hello\") a
  drop a
  drop b
  println (show n))
";
    let (out, result) = run(src);
    result.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out, "5\n");
    let (_, stats) = run_stats(src);
    let s = stats.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!((s.completions, s.parks, s.cancels), (1, 1, 0));
}
