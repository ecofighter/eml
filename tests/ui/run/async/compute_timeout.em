-- `timeout` で囲んだ `compute` は、仕事が終わってからでないと戻らない。過ぎた期限の取り消しは仕事の後で届いて
-- `None` になり、期限の前に終わった仕事は `Some` で返る (docs/spec/stdlib.md の「`compute`」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () -> spin 40)
  let _ = spin 40
  println (show (Async.timeout 100 (fn () -> Async.compute (fn () -> spin 40 + 1))))
  println (show (Async.timeout 0 (fn () -> Async.compute (fn () -> spin 40 + 9))))
  println (show (Async.timeout 100 (fn () -> Async.compute (fn () -> spin 40 + 2))))
  println (show (Async.await t))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
