-- 妥当でない UTF-8 を `String.decode` に渡すと、最初の妥当でないバイトの位置を持つ `Panic` を起こす。
import Bytes
import String

main : Unit -> <IO, Panic> Unit
main () =
  let b = Bytes.append (Bytes.of_string "abc") (Bytes.slice 0 1 (Bytes.of_string "é"))
  println (String.decode b)
