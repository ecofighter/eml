-- 負の長さは `Panic` を起こし、捕まえなければ実行時エラーになる。
import Bytes

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Bytes.length (Bytes.make (-1) 0u8)))
