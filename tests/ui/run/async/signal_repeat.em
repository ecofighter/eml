-- signal: interrupt at 5
-- signal: interrupt at 10
-- 起こされたタスクは次の `wait` で次のシグナルを待てる。2つの予定はそれぞれ1回の待ちを起こす
-- (docs/spec/stdlib.md の「`Std.Signal`」)。
import Async
import Signal

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () ->
    Signal.wait Signal.Interrupt
    println "first"
    Signal.wait Signal.Interrupt
    "second")
  let _ = spin 40
  println (Async.await t))
