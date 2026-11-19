-- 捕まえられなかった子の`Panic`は、両方の子が終わった後で`par`が起こし直す。左の子のものが先になる。誤りは
-- stdの`panic`の位置を持つ (docs/spec/stdlib.mdの「`Std.Par`」)。
import Std.Par

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  let (a, b) = Par.par (fn () -> spin 40 + 1 / 0) (fn () -> (panic "right" : Int))
  println (show (a + b))
