-- signal: interrupt at 5
-- 1つのシグナルは、そのとき同じシグナルを待つタスクをすべて起こす (docs/spec/stdlib.md の「`Std.Signal`」)。
import Async
import Signal

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  let a = Async.start (fn () ->
    Signal.wait Signal.Interrupt
    "a woke")
  let b = Async.start (fn () ->
    Signal.wait Signal.Interrupt
    "b woke")
  let _ = spin 40
  println (Async.await a)
  println (Async.await b)
