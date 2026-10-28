-- `cancel` は待っているタスクを取り消し、終わったタスクのハンドルなら残っている `Lin` の結果を落とす。どちらも
-- 検査モードで漏れが出ない (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

body : Unit -> <IO, Panic, Async> Unit
body () =
  let done = Async.start (fn () -> MutArray.make 3 0)
  let _ = spin 40
  let t = Async.start (fn () ->
    match try_io forever with
      | Ok n -> println "unreachable \{show n}"
      | Err e -> println "t: \{show e.kind}: \{e}")
  let _ = spin 40
  let n = Async.await (Async.start (fn () -> spin 40 + 5))
  println "awaited \{show n}"
  Async.cancel done
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
