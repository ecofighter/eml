import Std.Par
import Std.MutArray
import Std.Array

main : Unit -> <IO, Panic> Unit
main () =
  let m = MutArray.make 3 0
  let (m, n) = Par.par (fn () -> MutArray.write m 0 5) (fn () -> 3)
  let (x, m) = MutArray.read m 0
  println (show (x + n + Array.length (MutArray.freeze m)))
