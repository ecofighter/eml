-- `Bytes.find needle b` は最初に見つかった位置を返す。空の列は 0 で見つかる。
import Bytes

main : Unit -> <IO, Panic> Unit
main () =
  let b = Bytes.of_string "abcabc"
  println (show (Bytes.find (Bytes.of_string "bc") b))
  println (show (Bytes.find (Bytes.of_string "ca") b))
  println (show (Bytes.find (Bytes.of_string "cc") b))
  println (show (Bytes.find (Bytes.empty ()) b))
  println (show (Bytes.find b (Bytes.of_string "abc")))
  println (show (Bytes.find (Bytes.of_string "x") (Bytes.empty ())))
