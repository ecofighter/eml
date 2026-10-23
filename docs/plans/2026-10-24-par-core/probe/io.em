import Std.Par

noisy : Unit -> <IO> Int
noisy () =
  println "x"
  1

main : Unit -> <IO> Unit
main () =
  let (a, b) = Par.par noisy (fn () -> 3)
  println (show (a + b))
