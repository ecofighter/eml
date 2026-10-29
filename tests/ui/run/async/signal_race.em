-- signal: terminate at 10
-- 予定した `Terminate` が `sleep_ms 1000` より先に来るので、`race` は `Signal.wait` の側を勝たせる
-- (docs/spec/stdlib.md の「`Std.Signal`」)。
import Async
import Signal

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () ->
    Async.race (fn () -> Async.sleep_ms 1000) (fn () -> Signal.wait Signal.Terminate))
  let _ = spin 40
  match Async.await t with
    | Left () -> println "slept"
    | Right () -> println "terminated")
