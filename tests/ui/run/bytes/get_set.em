-- `get` は位置のバイトを返し、`set` は一意ならその場で、共有していれば写して書き換える。`make` は同じバイトで埋める。
-- `Show Bytes` は要素を `u8` なしの数で並べる。
import Bytes

main : Unit -> <IO, Panic> Unit
main () =
  let b = Bytes.make 3 7u8
  println (show b)
  let c = Bytes.set 1 200u8 b
  println (show c)
  println (show (Bytes.get 1 c))
  println (show (Bytes.get 0 c))
  println (show (Bytes.make 0 1u8))
  let shared = Bytes.of_string "abc"
  let changed = Bytes.set 0 120u8 shared
  println (show shared)
  println (show changed)
  println (show (shared == changed))
