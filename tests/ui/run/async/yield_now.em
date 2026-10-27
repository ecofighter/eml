-- `yield_now` はタスクを実行待ちの列の後ろへ回して戻り、どのタスクも最後まで動く (docs/spec/stdlib.md の「`Std.Async`」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

count : Int -> Int -> <IO, Panic, Async> Int
count n acc = if n == 0 then acc else step n acc

step : Int -> Int -> <IO, Panic, Async> Int
step n acc =
  Async.yield_now ()
  count (n - 1) (acc + n)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let a = Async.start (fn () -> count 3 0)
  let b = Async.start (fn () -> count 4 0)
  let c = Async.start (fn () -> count 5 0)
  let _ = spin 40
  let x = Async.await a
  let y = Async.await b
  let z = Async.await c
  println "\{show x} \{show y} \{show z}"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
