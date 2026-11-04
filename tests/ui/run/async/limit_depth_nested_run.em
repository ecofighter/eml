-- max-depth: 4096
-- タスクの中の入れ子の `run` の本体の深さの超過は、`run` が起こし直し、外のタスクが捕まえる
-- (docs/spec/vm.md の「`Async` のタスク」)。
import Async (Async)
import String

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

dive : Int -> <Panic> Int
dive n =
  let label = show n
  let below = dive (n + 1)
  below + String.byte_length label

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    match try_io (fn () -> Async.run (fn () -> dive 0)) with
      | Ok n -> println "unreachable \{show n}"
      | Err e -> println "outer task caught: \{show e.kind}: \{e}")
  let _ = spin 40
  Async.await t
  println "done"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
