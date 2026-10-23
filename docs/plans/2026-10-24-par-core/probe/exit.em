import Std.Par

main : Unit -> <IO> Unit
main () =
  let (a, b) = Par.par (fn () -> exit 1) (fn () -> 3)
  println (show (a + b))
