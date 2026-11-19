-- `Lin`の値は`await`とスコープを通って呼んだ側に届く (docs/spec/stdlib.mdの「`Std.Async`」)。
import Async
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  let a = Async.scope (fn _ ->
    let t = Async.start (fn () -> MutArray.make 3 7)
    let _ = spin 40
    Async.await t)
  let (n, a) = MutArray.length a
  let (x, a) = MutArray.read a 0
  drop a
  println "\{show n} \{show x}"
