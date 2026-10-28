-- `drain_within` の待ちは取り消しの地点で、取り消された呼び手は `s` のタスクを取り消さずに `Cancelled` を起こす。
-- `shield` の中でも、期限は `s` のタスクを取り消す (docs/spec/stdlib.md の「時間」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

worker : String -> Int -> Unit -> <IO, Panic, Async> Unit
worker name ms () =
  match try_io (fn () -> Async.sleep_ms ms) with
    | Ok () -> println "\{name}: done"
    | Err e -> println "\{name}: \{show e.kind}"

drainer : Async.Scope -> Unit -> <IO, Panic, Async> Unit
drainer s () =
  match try_io (fn () -> Async.drain_within 200 s) with
    | Ok () -> println "unreachable"
    | Err e -> println "drainer: \{show e.kind}"

-- 取り消した待ちを残したまま、何度も待ち直せる
drainers : Int -> Async.Scope -> Async.Scope -> <IO, Panic, Async> Unit
drainers n o s =
  if n == 0 then ()
  else
    let d = Async.start_in o (drainer s)
    Async.sleep_ms 10
    Async.cancel d
    drainers (n - 1) o s

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.scope (fn o ->
    Async.scope (fn s ->
      Async.spawn_in s (worker "kept" 100)
      let _ = spin 40
      drainers 3 o s
      Async.drain_within 200 s
      println "drained after 3 cancelled drains"))
  Async.scope (fn s ->
    Async.spawn_in s (worker "e" 300)
    let _ = spin 40
    Async.shield (fn () -> Async.drain_within 50 s)
    println "the shielded drain returned")

main : Unit -> <IO, Panic> Unit
main () = Async.run body
