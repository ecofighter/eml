-- `drain_within ms s` は `s` のタスクが終わるのを最大 `ms` ミリ秒待つ。残っていれば、`s` のタスクと、`s` が空になる
-- までに加わるタスクを取り消し、終わるまで待つ。呼んだタスクと `s` の本体は取り消さず、空になった後に加えたタスクは
-- 動く。離れたスコープではすぐに戻り、過ぎた期限ではすぐに取り消す (docs/spec/stdlib.md の「時間」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

worker : String -> Int -> Unit -> <IO, Panic, Async> Unit
worker name ms () =
  match try_io (fn () -> Async.sleep_ms ms) with
    | Ok () -> println "\{name}: done"
    | Err e -> println "\{name}: \{show e.kind}"

stubborn : Async.Scope -> Unit -> <IO, Panic, Async> Unit
stubborn s () =
  match try_io (fn () -> Async.sleep_ms 300) with
    | Ok () -> println "c: done"
    | Err e ->
      println "c: \{show e.kind}"
      Async.spawn_in s (worker "late" 10)

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.scope (fn s ->
    Async.spawn_in s (worker "a" 30)
    let _ = spin 40
    Async.drain_within 100 s
    println "drained before the deadline")
  Async.scope (fn s ->
    Async.spawn_in s (worker "b" 30)
    let _ = spin 40
    Async.spawn_in s (stubborn s)
    Async.drain_within 100 s
    println "after the deadline"
    Async.yield_now ()
    let again = Async.start_in s (worker "again" 10)
    Async.await again
    println "the body is not cancelled")
  let gone = Async.scope (fn s -> s)
  Async.drain_within 100 gone
  println "a left scope returns at once"
  Async.scope (fn s ->
    Async.spawn_in s (worker "d" 30)
    let _ = spin 40
    Async.drain_within 0 s
    println "a past deadline cancels at once")

main : Unit -> <IO, Panic> Unit
main () = Async.run body
