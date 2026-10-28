-- 取り消されたタスクを待つと、`await` は `Cancelled` を起こし直す。待った側は取り消されていないので、
-- `cancel_requested` は偽で、run も失敗しない (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    Async.scope (fn s ->
      Async.cancel_scope s
      Async.yield_now ()))
  let _ = spin 40
  match try_io (fn () -> Async.await t) with
    | Ok () -> println "unreachable"
    | Err e -> println "await: \{show e.kind}: \{e}"
  println "requested \{show (Async.cancel_requested ())}"

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
