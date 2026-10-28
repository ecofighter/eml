-- E1058: `Std.Async` の `Async` は抽象エフェクトで、利用者は生の操作を handler の節に書けない。待たずに再開して、
-- 終わっていないタスクの結果を取りに行く道を名前解決で断つ (docs/spec/effects.md の「抽象エフェクト」)。
import Async

main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start (fn () -> 1)
    let x = handle Async.await t with
      | ctl Async.await_cell c k -> k Async.Woke
      | ctl Async.reschedule () k -> k Async.Woke
      | ctl Async.await_time ms k -> k Async.Woke
      | return x -> x
    println (show x))
