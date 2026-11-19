-- 1つ目のスコープが返したハンドルを、後のスコープで待てる。タスクは終わっているので`await`はすぐ値を返す
-- (docs/spec/stdlib.mdの「`Std.Async`」)。
import Async

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  let t = Async.scope (fn _ ->
    let t = Async.start (fn () -> spin 40 + 6)
    let _ = spin 40
    t)
  println (show (Async.scope (fn _ -> Async.await t * 7)))
