-- `drain_within` で待つタスクが、期限と同じ時刻に外から取り消される。取り消された呼び手は `s` に触れないので、
-- `s` のタスクは最後まで動く (docs/spec/stdlib.md の「時間」)。
import Async (Async)

worker : String -> Int -> Unit -> <IO, Panic, Async> Unit
worker name ms () =
  match try_io (fn () -> Async.sleep_ms ms) with
    | Ok () -> println "\{name}: done"
    | Err e -> println "\{name}: \{show e.kind}"

drainer : Async.Scope -> Unit -> <IO, Panic, Async> Unit
drainer s () =
  match try_io (fn () -> Async.drain_within 100 s) with
    | Ok () -> println "drainer: returned"
    | Err e -> println "drainer: \{show e.kind}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.scope (fn s ->
    Async.spawn_in s (worker "m" 300)
    Async.scope (fn o ->
      Async.scope (fn q ->
        Async.spawn_in o (fn () ->
          Async.sleep_ms 100
          Async.cancel_scope q)
        Async.spawn_in q (drainer s))))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
