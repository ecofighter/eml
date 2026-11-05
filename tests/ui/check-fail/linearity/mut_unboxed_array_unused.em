-- `MutUnboxedArray` は `Lin` なので、使わずに捨てると E3003
import MutUnboxedArray

main : Unit -> <IO, Panic> Unit
main () =
  let m = MutUnboxedArray.make 3 0.0
  println "unused"
