-- 範囲の外の切り出しは `Panic` を起こし、捕まえなければ実行時エラーになる。
import Bytes

main : Unit -> <IO, Panic> Unit
main () =
  let b = Bytes.of_string "abc"
  println (show (Bytes.length (Bytes.slice 2 9 b)))
