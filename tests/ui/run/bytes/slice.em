-- `Bytes.slice from to b` は範囲を写した新しいバイト列を返す。元のバイト列は変わらない。
import Bytes

main : Unit -> <IO, Panic> Unit
main () =
  let b = Bytes.of_string "hello world"
  println (show (Bytes.slice 0 5 b == Bytes.of_string "hello"))
  println (show (Bytes.slice 6 11 b == Bytes.of_string "world"))
  println (show (Bytes.length (Bytes.slice 3 3 b)))
  println (show (Bytes.slice 0 11 b == b))
  println (show (Bytes.length b))
  println (show (try_panic (fn () -> Bytes.length (Bytes.slice 2 12 b))))
  println (show (try_panic (fn () -> Bytes.length (Bytes.slice (-1) 2 b))))
  println (show (try_panic (fn () -> Bytes.length (Bytes.slice 4 3 b))))
  println (show (Bytes.length b))
