-- E2020: `par` の子は `IO` を起こせない。
import Std.Par

noisy : Unit -> <IO> Int
noisy () =
  println "x"
  1

main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par noisy (fn () -> 2)
  println (show (a + b))
