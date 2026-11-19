-- E3002: `Task a` は `Lin` で、`await` が消費する。同じハンドルを2回待つことはできない
-- (docs/spec/stdlib.md の「`Std.Async`」)。
import Async

main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  let t = Async.start (fn () -> 1)
  let x = Async.await t
  let y = Async.await t
  println (show (x + y))
