-- `cancel t` は、t が開いているスコープの子も取り消す。t が子を待つ `scope` の終わりは shield の中の待ちだが、
-- 子が取り消されるので終わる。ビットを立てた後に開くスコープも、開くときに取り消す (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

worker : Unit -> <IO, Panic, Async> Unit
worker () =
  Async.scope (fn s ->
    Async.spawn_in s (fn () ->
      let _ = forever ()
      ())
    let _ = spin 40
    ())
  match try_io (fn () -> Async.yield_now ()) with
    | Ok () -> println "unreachable"
    | Err e -> println "worker: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start worker
  let _ = spin 40
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
