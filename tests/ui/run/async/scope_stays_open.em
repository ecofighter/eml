-- 名前付きのスコープは、本体が動いている間は閉じない。先に置いた子が終わってタスクの一覧が空になっても、本体は
-- 同じスコープに続けてタスクを置ける (docs/spec/stdlib.md の「スコープ」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

inner : Async.Scope -> <IO, Panic, Async> Int
inner s =
  Async.spawn_in s (fn () -> println "first")
  let _ = spin 40
  Async.yield_now ()
  Async.yield_now ()
  let t = Async.start (fn () -> 2)
  let _ = spin 40
  Async.await t

body : Unit -> <IO, Panic, Async> Unit
body () =
  let n = Async.scope inner
  println (show n)

main : Unit -> <IO, Panic> Unit
main () = Async.run body
