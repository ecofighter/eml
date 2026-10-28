-- `cancel t` は、t の中の入れ子の `run` のタスクも取り消す。入れ子の `run` は `Cancelled` を起こし直す
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

worker : Unit -> <IO, Panic, Async> Unit
worker () =
  let r = try_io (fn () ->
    Async.run (fn () ->
      let u = Async.start forever
      let _ = spin 40
      Async.await u))
  match r with
    | Ok n -> println "unreachable \{show n}"
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
