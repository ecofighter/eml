-- 子が `cancel_scope` でスコープを取り消すと、待っている本体も旗で取り消され、`Cancelled` が `scope` から出る。
-- `scope` を囲む `try_io` で捕まえれば、スコープを出たタスクはもう取り消されていない (docs/spec/stdlib.md の「スコープ」)。
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
  Async.spawn (fn () ->
    let _ = spin 400
    Async.cancel_scope s)
  let _ = spin 40
  Async.await u

body : Unit -> <IO, Panic, Async> Unit
body () =
  match try_io (fn () -> Async.scope inner) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "scope: \{show e.kind}: \{e}"
  Async.yield_now ()
  println "still running"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
