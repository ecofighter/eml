-- E2002: `compute`の仕事のrowに`Async`の操作を行う関数は入らない (docs/spec/stdlib.mdの「`compute`」)。
import Async (Async)

main : Unit -> <IO, Panic, Async> Unit
main () =
  Async.compute (fn () -> Async.yield_now ())
