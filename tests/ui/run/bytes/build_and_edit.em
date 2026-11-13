import Byte
import Bytes

main : Unit -> <IO, Panic> Unit
main () =
  let b = Bytes.with_capacity 4
  let b = Bytes.push (Byte.of_int 1) b
  let b = Bytes.push (Byte.of_int 2) b
  let b = Bytes.set 0 (Byte.of_int 5) b
  let b = Bytes.truncate 1 b
  println (show (Bytes.length b))
  println (show (Byte.to_int (Bytes.get 0 b)))
