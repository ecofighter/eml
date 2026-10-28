-- 期限は本体が始めたタスクにも届く。`timeout` は、本体の入れ子のスコープのタスクと本体が始めたタスクがすべて
-- 終わってから戻り、親のない子を残さない (docs/spec/stdlib.md の「時間」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

worker : String -> Int -> Unit -> <IO, Panic, Async> Unit
worker name ms () =
  match try_io (fn () -> Async.sleep_ms ms) with
    | Ok () -> println "\{name}: done"
    | Err e -> println "\{name}: \{show e.kind}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let r = Async.timeout 50 (fn () ->
    Async.scope (fn s ->
      Async.spawn_in s (worker "scoped" 100)
      let _ = spin 40
      Async.sleep_ms 200
      1))
  println "timeout: \{show r}"
  let r = Async.timeout 50 (fn () ->
    Async.spawn (worker "spawned" 100)
    let _ = spin 40
    2)
  println "timeout: \{show r}"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
