-- 子の中の `Panic` は、子の中の `try_panic` か、親の `try_panic` に届く。親に届く `Panic` は、ほかの中断と同じく
-- join の後で起こし直す (docs/spec/vm.md の「`par` の fork と join」)。
import Std.Par

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> spin 40 + 1) (fn () -> try_panic (fn () -> 1 / 0))
  println (show (a, b))
  let r1 = try_panic (fn () -> Par.par (fn () -> spin 40 + 1 / 0) (fn () -> 2))
  println (show r1)
  let r2 = try_panic (fn () -> Par.par (fn () -> spin 40 + 1) (fn () -> (panic "boom" : Int)))
  println (show r2)
