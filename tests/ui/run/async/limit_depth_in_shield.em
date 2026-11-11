-- native: skip (the depth limit is the thread's stack, not a word count)
-- max-depth: 4096
-- `shield` の中の深さの超過も `ResourceLimit` になり、`shield` の `reraise` が外の `try_all` に届ける
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)
import String

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

dive : Int -> <Panic> Int
dive n =
  let label = show n
  let below = dive (n + 1)
  below + String.byte_length label

describe : Result Async.Failure Int -> String
describe r = match r with
  | Ok n -> "ok \{show n}"
  | Err (Async.Panicked m) -> "panicked: \{m}"
  | Err (Async.IOFailed e) -> "io failed: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    Async.try_all (fn () -> Async.shield (fn () -> dive 0)))
  let _ = spin 40
  println (describe (Async.await t))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
