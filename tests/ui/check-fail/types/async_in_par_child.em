-- E2020: `par` の子は `Async` のタスクを動かせない。`yield_now` の row `<IO, Async>` は `never` の操作だけではないので、
-- 子の row に入らない。診断は row の最初の効果 `IO` を名指す (docs/spec/stdlib.md の「`Std.Par`」)。
import Std.Par
import Async

main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let (a, b) = Par.par (fn () -> Async.yield_now ()) (fn () -> 1)
    println (show b))
