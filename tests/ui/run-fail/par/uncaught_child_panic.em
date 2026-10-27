-- 捕まえられなかった子の `Panic` は、両方の子が終わった後で、いちばん左の子のものを起こし直す。起こし直す関数の
-- 名前で報告し、ソースの位置を持たない (docs/implementation/status.md の「`par`」)。
import Std.Par

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  let (a, b) = Par.par (fn () -> spin 40 + 1 / 0) (fn () -> (panic "right" : Int))
  println (show (a + b))
