-- いくつものタスクがそれぞれ `compute` で待つ間も、worker はほかのタスクと仕事を動かす
-- (docs/spec/stdlib.md の「`compute`」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

sum : Int -> Int -> <Panic> Int
sum acc n = if n == 0 then acc else sum (acc + n) (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () -> spin 40)
  let _ = spin 40
  let a = Async.start (fn () -> Async.compute (fn () -> sum 0 10))
  let b = Async.start (fn () -> Async.compute (fn () -> sum 0 20))
  let c = Async.start (fn () -> Async.compute (fn () -> sum 0 30))
  let d = Async.start (fn () -> Async.compute (fn () -> sum 0 40))
  println (show (Async.await a))
  println (show (Async.await b))
  println (show (Async.await c))
  println (show (Async.await d))
  println (show (Async.await t))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
