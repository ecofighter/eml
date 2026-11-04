-- max-depth: 4096
-- スコープに取り消されたタスクが `Cancelled` を受けた後でも、深さの超過は `ResourceLimit` として受け取れる
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

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

describe : Result Async.Failure Int -> String
describe r = match r with
  | Ok n -> "ok \{show n}"
  | Err (Async.Panicked m) -> "panicked: \{m}"
  | Err (Async.IOFailed e) -> "io failed: \{show e.kind}: \{e}"

worker : Unit -> <IO, Panic, Async> Unit
worker () =
  println (describe (Async.try_all forever))
  println (describe (Async.try_all (fn () -> dive 0)))

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.scope (fn s ->
    Async.spawn_in s worker
    let _ = spin 40
    Async.cancel_scope s)

main : Unit -> <IO, Panic> Unit
main () = Async.run body
