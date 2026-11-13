import Array
import MutArray

main : Unit -> <IO, Panic> Unit
main () =
  let a = Array.make 3 0
  let a = Array.set a 1 5
  println (show (Array.get a 1))
  println (show (Array.get a 0))
  let m = Array.thaw a
  let (n, m) = MutArray.length m
  println (show n)
  println (show (Array.length (MutArray.freeze m)))
