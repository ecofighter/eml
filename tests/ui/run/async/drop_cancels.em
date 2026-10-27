-- `drop` で落としたハンドルのタスクは取り消される。待っていた `await` は `Cancelled` で戻り、std の包みが種類
-- `Cancelled` の IO の失敗を投げる。待たれていたタスクも、包みがハンドルを落とすので取り消される
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

victim : Unit -> <IO, Panic, Async> Unit
victim () =
  let u = Async.start forever
  let _ = spin 40
  match try_io (fn () -> Async.await u) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "victim: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start victim
  let _ = spin 40
  Async.yield_now ()
  drop t

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
