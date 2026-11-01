import Std.Par

main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> 1) (fn () -> 2)
  println (show (a + b))
