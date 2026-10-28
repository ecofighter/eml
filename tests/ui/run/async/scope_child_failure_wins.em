-- スコープの子の失敗は本体を取り消す。本体の `Cancelled` は記録しないので、`scope` は子の失敗を起こし直す。スコープを
-- 出たタスクは取り消されておらず、続けて動く (docs/spec/stdlib.md の「スコープ」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

inner : Async.Scope -> <IO, Panic, Async> Int
inner s =
  let u = Async.start forever
  let _ = spin 40
  Async.spawn_in s (fn () ->
    let _ = spin 400
    fail_io TimedOut "too slow")
  let _ = spin 40
  Async.await u

body : Unit -> <IO, Panic, Async> Int
body () =
  let r = try_io (fn () -> Async.scope inner)
  match r with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "scope: \{show e.kind}: \{e}"
  let t = Async.start (fn () -> spin 40 + 1)
  let _ = spin 40
  Async.await t

main : Unit -> <IO, Panic> Unit
main () = println (show (Async.run body))
