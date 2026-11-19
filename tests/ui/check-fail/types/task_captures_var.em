-- E2002: タスクは閉じた row `<IO, Panic, Async>` で始まり、始めた側の `var` は見えない。`var` を捕まえると型の誤りになる
-- (docs/spec/stdlib.md の「`Std.Async`」)。
import Async

main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  var n = 0
  let t = Async.start (fn () -> n <- n + 1)
  Async.await t
  println (show n)
