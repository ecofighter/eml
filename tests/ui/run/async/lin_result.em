-- `Lin` の値は `await` と `run` を通って呼んだ側に届く (docs/spec/stdlib.md の「`Std.Async`」)。
import Async
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let a = Async.run (fn () ->
    let t = Async.start (fn () -> MutArray.make 3 7)
    let _ = spin 40
    Async.await t)
  let (n, a) = MutArray.length a
  let (x, a) = MutArray.read a 0
  drop a
  println "\{show n} \{show x}"
