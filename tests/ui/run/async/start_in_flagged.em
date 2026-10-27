-- 旗を立てたスコープに `start_in` で加えたタスクは、作るときに取り消される。最初の取り消しの地点で投げる
-- (docs/spec/stdlib.md の「スコープ」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

inner : Async.Scope -> <IO, Panic, Async> Unit
inner s =
  Async.cancel_scope s
  let t = Async.start_in s (fn () ->
    println "child starts"
    match try_io (fn () -> Async.yield_now ()) with
      | Ok () -> println "unreachable"
      | Err e -> println "child: \{show e.kind}: \{e}")
  let _ = spin 40
  Async.await t

body : Unit -> <IO, Panic, Async> Unit
body () =
  match try_io (fn () -> Async.scope inner) with
    | Ok () -> println "unreachable"
    | Err e -> println "scope: \{show e.kind}: \{e}"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
