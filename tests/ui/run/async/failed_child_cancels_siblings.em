-- 失敗した子は、同じスコープのほかのタスクを取り消す。`await` で待つ兄弟も、`yield_now` で回り続ける兄弟も
-- 取り消され、`run` は最初の失敗を起こし直す (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

waiter : Unit -> <IO, Panic, Async> Unit
waiter () =
  let u = Async.start forever
  let _ = spin 40
  match try_io (fn () -> Async.await u) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "waiter: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.spawn waiter
  let _ = spin 40
  Async.spawn (fn () ->
    let _ = forever ()
    ())
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
