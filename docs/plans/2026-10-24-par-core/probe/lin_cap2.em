import Std.Par
import Std.MutArray

main : Unit -> <IO, Panic> Unit
main () =
  let m = MutArray.make 3 0
  let (u, n) = Par.par (fn () -> MutArray.freeze (MutArray.write m 0 5)) (fn () -> 3)
  println (show n)
