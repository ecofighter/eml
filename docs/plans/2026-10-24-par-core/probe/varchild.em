import Std.Par

main : Unit -> <IO, Panic> Unit
main () =
  var x = 1
  let (a, b) = Par.par (fn () -> x) (fn () -> 3)
  println (show (a + b))
