-- signal: interrupt at 20
-- `wait` は取り消しの地点で、取り消された待ちは列から外れる。シグナルより先に来た期限で `timeout` は `None` を
-- 返し、その後の新しい待ちがシグナルで起きる (docs/spec/stdlib.md の「`Std.Signal`」)。
import Async
import Signal

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () ->
    println (show (Async.timeout 5 (fn () -> Signal.wait Signal.Interrupt)))
    Signal.wait Signal.Interrupt
    println "woke")
  let _ = spin 40
  Async.await t)
