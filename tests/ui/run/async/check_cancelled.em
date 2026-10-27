-- 待たない純粋なループも、`check_cancelled` を呼べば取り消せる。予算を使い切るたびにほかのタスクへ worker を渡すので、
-- 取り消すタスクも動ける (docs/spec/stdlib.md の「取り消し」「操作の予算」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

busy : Int -> <IO, Panic, Async> Int
busy n =
  Async.check_cancelled ()
  busy (n + 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    match try_io (fn () -> busy 0) with
      | Ok n -> println "unreachable \{show n}"
      | Err e -> println "busy: \{show e.kind}: \{e}")
  let _ = spin 40
  Async.yield_now ()
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () = Async.run body
