-- native: skip (the depth limit is the thread's stack, not a word count)
-- max-depth: 4096
-- spawn したタスクの深さの超過はスコープの失敗になり、`run` が `ResourceLimit` の IO の失敗として起こし直す
-- (docs/spec/stdlib.md の「`Std.Async`」)。
import Async (Async)
import String

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

dive : Int -> <Panic> Int
dive n =
  let label = show n
  let below = dive (n + 1)
  below + String.byte_length label

body : Unit -> <IO, Panic, Async> Int
body () =
  Async.spawn (fn () ->
    let _ = spin 40
    let n = dive 0
    println "unreachable \{show n}")
  let _ = spin 40
  1

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Async.run body) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "run failed: \{show e.kind}: \{e}"
