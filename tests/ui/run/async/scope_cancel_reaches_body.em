-- 本体がスコープの外のタスクを待っていても、子が `cancel_scope` でスコープを取り消すと、本体の待ちは旗で外れ、
-- `Cancelled` が `scope` から出る。待たれていたタスクは、包みがハンドルを落とすので取り消される
-- (docs/spec/stdlib.md の「スコープ」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

body : Unit -> <IO, Panic, Async> Unit
body () =
  let u = Async.start forever
  let _ = spin 40
  let inner = fn s ->
    Async.spawn_in s (fn () ->
      let _ = spin 400
      Async.cancel_scope s)
    let _ = spin 40
    Async.await u
  match try_io (fn () -> Async.scope inner) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "scope: \{show e.kind}"
  println "done"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
