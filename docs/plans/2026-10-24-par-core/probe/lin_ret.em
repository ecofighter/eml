import Std.Par
import Std.MutArray
import Std.Array

main : Unit -> <IO, Panic> Unit
main () =
  let (m, n) = Par.par (fn () -> MutArray.make 3 4) (fn () -> 3)
  let (x, m) = MutArray.read m 0
  println (show (x + n + Array.length (MutArray.freeze m)))
