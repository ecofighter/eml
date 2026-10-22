-- `String.from_utf8` は妥当な UTF-8 だけを `String` にし、そうでなければ `None` を返す。
import Bytes
import String

main : Unit -> <IO, Panic> Unit
main () =
  let ok = Bytes.of_string "héllo"
  println (show (String.from_utf8 ok))
  println (show (String.from_utf8 (Bytes.empty ())))
  let bad = Bytes.append (Bytes.of_string "abc") (Bytes.slice 0 1 (Bytes.of_string "é"))
  println (show (Bytes.length bad))
  println (show (String.from_utf8 bad))
  println (show (String.decode (Bytes.of_string "日本")))
