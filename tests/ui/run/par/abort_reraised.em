-- 子の中断は、両方の子が終わった後で、親の handler に起こし直す。中断しなかった側の値は捨てる
-- (docs/spec/vm.md の「`par` の fork と join」)。
import Std.Par
import Fail (Fail(..))

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

upto : Int -> List Int -> <Panic> List Int
upto n acc = if n == 0 then acc else upto (n - 1) (n :: acc)

main : Unit -> <IO, Panic> Unit
main () =
  let r1 = Fail.to_result (fn () -> Par.par (fn () -> spin 40 + Fail.fail 1) (fn () -> upto 3 []))
  println (show r1)
  let r2 = Fail.to_result (fn () -> Par.par (fn () -> upto (spin 40 + 3) []) (fn () -> (Fail.fail 2 : Int)))
  println (show r2)
  let r3 = Fail.to_result (fn () -> Par.par (fn () -> upto (spin 40 + 2) []) (fn () -> (Fail.fail "right" : Int)))
  println (show r3)
  let r4 = Fail.to_result (fn () -> Par.par (fn () -> (Fail.fail "left" : Int)) (fn () -> "kept"))
  println (show r4)
