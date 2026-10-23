import Std.Par

noisy : Unit -> <IO> Int
noisy () =
  println "x"
  1

apply2 : ((Unit -> <e> Int) -> (Unit -> <e> Int) -> <e> (Int, Int)) -> (Unit -> <e> Int) -> <e> (Int, Int)
apply2 p f = p f f

main : Unit -> <IO, Panic> Unit
main () =
  let p = Par.par
  let (a, b) = p (fn () -> 1) (fn () -> 2)
  println (show (a + b))
  let (c, d) = apply2 Par.par noisy
  println (show (c + d))
