-- `shield` の中で開いたスコープは、開いた後に外から届いた取り消しでも印を付けられない。属するスコープを取り消すと
-- タスクのビットからの印付けも、スコープからたどる印付けも、`shield` の境目で止まる。`shield` を出た後の最初の
-- 取り消しの地点が、外の取り消しで投げる (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

worker : String -> Int -> Unit -> <IO, Panic, Async> Unit
worker name ms () =
  match try_io (fn () -> Async.sleep_ms ms) with
    | Ok () -> println "\{name}: done"
    | Err e -> println "\{name}: \{show e.kind}"

child : Unit -> <IO, Panic, Async> Unit
child () =
  let r = try_io (fn () ->
    Async.shield (fn () ->
      Async.scope (fn inner ->
        Async.spawn_in inner (worker "grandchild" 10)
        let _ = spin 40
        Async.yield_now ()
        println "shielded scope survives"))
    Async.yield_now ())
  match r with
    | Ok () -> println "unreachable"
    | Err e -> println "after the shield: \{show e.kind}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.scope (fn s ->
    Async.spawn_in s child
    let _ = spin 40
    Async.yield_now ()
    Async.cancel_scope s)

main : Unit -> <IO, Panic> Unit
main () = Async.run body
