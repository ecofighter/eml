-- E2020: `compute` の仕事の row に `Async` の操作を行う関数は入らない。仕事の row が `Async` を含むと、E2020 に
-- E2002 が並ぶ (docs/spec/stdlib.md の「`compute`」)。
import Async (Async)

main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () -> Async.compute (fn () -> Async.yield_now ()))
