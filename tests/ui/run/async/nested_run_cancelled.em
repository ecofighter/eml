-- タスクの中の入れ子の `run` の根は、呼んだタスクのスコープの下にある。外のスコープが子の失敗で取り消されると、入れ子の
-- `run` のタスクも取り消され、`run` は `Cancelled` を起こし直す (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

nested : Unit -> <IO, Panic, Async> Unit
nested () =
  let r = try_io (fn () ->
    Async.run (fn () ->
      let u = Async.start forever
      let _ = spin 40
      Async.await u))
  match r with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "inner run: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.spawn nested
  let _ = spin 40
  Async.spawn (fn () ->
    let _ = spin 400
    fail_io TimedOut "too slow")
  let _ = spin 40
  ()

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Async.run body) with
    | Ok () -> println "unreachable"
    | Err e -> println "run: \{show e.kind}: \{e}"
