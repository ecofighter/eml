-- 子の中の mask は、子の evidence の列で親の列と同じ数だけ evidence を飛ばす。`foo` の mask は内側の `Fail` の
-- handler を飛ばすので、子の中断は2つ目の `Fail` の handler に届き、深さ 1 で親に起こし直す
-- (docs/spec/vm.md の「`par` の fork と join」)。
import Std.Par
import Fail (Fail(..))

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

foo : (Unit -> <never e> Int) -> <Fail String | e> Int
foo h = h ()

main : Unit -> <IO, Panic> Unit
main () =
  let r = Fail.to_result (fn () -> Fail.to_result (fn () -> Par.par (fn () -> spin 40 + foo (fn () -> Fail.fail 7)) (fn () -> 1)))
  println (show r)
