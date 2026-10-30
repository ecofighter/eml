-- `with_capacity` は空のバイト列を返し、`push` で長さが伸びる。容量は長さに出ない
import Bytes

main : Unit -> <IO, Panic> Unit
main () =
  let b = Bytes.with_capacity 16
  println (show (Bytes.length b))
  let c = Bytes.push 2u8 (Bytes.push 1u8 b)
  println (show (Bytes.length c))
  println (show c)
  println (show (Bytes.length (Bytes.with_capacity 0)))
