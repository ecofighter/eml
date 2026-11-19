-- 両方の子が`Panic`を起こしたときは、右の子が先に終わっても、左の子の`Panic`を起こし直す
-- (docs/spec/stdlib.mdの「`Std.Par`」)。
import Std.Par

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let r1 = try_panic (fn () -> Par.par (fn () -> spin 40 + panic "left") (fn () -> (panic "right" : Int)))
  println (show r1)
  let r2 = try_panic (fn () -> Par.par (fn () -> spin 40 + panic "left") (fn () -> 1 / 0))
  println (show r2)
  let r3 = try_panic (fn () -> Par.par (fn () -> spin 40 + 1 / 0) (fn () -> (panic "right" : Int)))
  println (show r3)
