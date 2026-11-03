-- 仕事の中の never の操作は、仕事が終わった後に呼んだタスクで、仕事が届いた handler に起こし直す。`foo` の mask は
-- 内側の `Fail` の handler を飛ばすので、仕事の中断は2つ目の `Fail` の handler に届く。`Std.Par` を import しない
-- プログラムでも起こし直せる (docs/spec/stdlib.md の「`compute`」)。
import Async (Async)
import Fail (Fail(..))

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

foo : (Unit -> <never e> Int) -> <Fail String | e> Int
foo h = h ()

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () -> spin 40)
  let _ = spin 40
  let r = Fail.to_result (fn () -> Async.compute (fn () -> spin 40 + Fail.fail 1))
  println (show r)
  let m = Fail.to_result (fn () -> Fail.to_result (fn () -> Async.compute (fn () -> spin 40 + foo (fn () -> Fail.fail 7))))
  println (show m)
  let v = Fail.to_result (fn () -> Async.compute (fn () -> if spin 40 == 0 then 2 else Fail.fail 3))
  println (show v)
  println (show (Async.await t))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
