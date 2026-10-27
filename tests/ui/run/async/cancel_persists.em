-- 取り消しは続く。最初の `Cancelled` を `try_io` で捕まえても、次の取り消しの地点がまた投げる
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

stubborn : Unit -> <IO, Panic, Async> Unit
stubborn () =
  let u = Async.start forever
  let _ = spin 40
  match try_io (fn () -> Async.await u) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "first: \{show e.kind}: \{e}"
  match try_io (fn () -> Async.yield_now ()) with
    | Ok () -> println "unreachable"
    | Err e -> println "second: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start stubborn
  let _ = spin 40
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () = Async.run body
