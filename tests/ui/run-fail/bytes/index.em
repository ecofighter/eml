-- 範囲の外の位置は `Panic` を起こし、捕まえなければ実行時エラーになる。
import Bytes

main : Unit -> <IO, Panic> Unit
main () =
  let b = Bytes.make 3 1u8
  println (show (Bytes.get 5 b))
