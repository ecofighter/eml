import Std.Par

main : Unit -> <IO, Panic> Unit
main () =
  let (x, n) = Par.par (fn () -> 1.5 * 2.0) (fn () -> 40 + 2)
  println (show x)
  println (show n)
  let p = Par.par (fn () -> 0.25)
  let (y, m) = p (fn () -> 7)
  println (show (y, m))
