-- `Eq Bytes` と `Ord Bytes`。順序はバイトの辞書式の順序で、先頭が共通なら短い方が先に来る。
import Bytes

main : Unit -> <IO, Panic> Unit
main () =
  let a = Bytes.of_string "abc"
  let b = Bytes.of_string "abd"
  let empty = Bytes.empty ()
  println (show (a == a))
  println (show (a == Bytes.of_string "abc"))
  println (show (a != b))
  println (show (compare a b))
  println (show (compare b a))
  println (show (compare a a))
  println (show (compare empty a))
  println (show (compare a (Bytes.of_string "ab")))
  println (show (compare (Bytes.of_string "é") (Bytes.of_string "z")))
  println (show (a < b))
  println (show (b <= a))
