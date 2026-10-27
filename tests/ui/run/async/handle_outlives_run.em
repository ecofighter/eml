-- 1つ目の `run` が返したハンドルを、同じ実行の後の `run` で待てる。タスクは終わっているので `await` はすぐ値を返す
-- (docs/spec/stdlib.md の「`Std.Async`」)。
import Async

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let t = Async.run (fn () ->
    let t = Async.start (fn () -> spin 40 + 6)
    let _ = spin 40
    t)
  println (show (Async.run (fn () -> Async.await t * 7)))
