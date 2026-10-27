-- 両方の子が中断すれば、いちばん左の子の中断を起こし直す。右の子が先に中断して終わっても変わらない
-- (docs/spec/vm.md の「`par` の fork と join」)。
import Std.Par
import Fail (Fail(..))

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let r1 = Fail.to_result (fn () -> Par.par (fn () -> spin 40 + Fail.fail "left") (fn () -> (Fail.fail "right" : Int)))
  println (show r1)
  let r2 = try_panic (fn () -> Fail.to_result (fn () -> Par.par (fn () -> spin 40 + Fail.fail "left") (fn () -> 1 / 0)))
  println (show r2)
  let r3 = try_panic (fn () -> Fail.to_result (fn () -> Par.par (fn () -> spin 40 + panic "l") (fn () -> (Fail.fail "right" : Int))))
  println (show r3)
