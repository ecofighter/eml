-- `drain_within` の期限と、`s` が空になるのが同じ時刻に来る。どちらが先かはスケジュールで決まるので、遅れて加わる
-- タスクは何も出力しない。どちらでも、戻った後に `s` に加えたタスクは普通に動く (docs/spec/stdlib.md の「時間」)。
import Async (Async)

worker : String -> Int -> Unit -> <IO, Panic, Async> Unit
worker name ms () =
  match try_io (fn () -> Async.sleep_ms ms) with
    | Ok () -> println "\{name}: done"
    | Err e -> println "\{name}: \{show e.kind}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.scope (fn s ->
    Async.spawn_in s (worker "a" 100)
    Async.scope (fn o ->
      Async.spawn_in o (fn () ->
        Async.sleep_ms 100
        Async.spawn_in s (fn () -> Async.shield (fn () -> Async.sleep_ms 50)))
      Async.drain_within 100 s
      println "drained"
      Async.spawn_in s (worker "after" 10)))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
