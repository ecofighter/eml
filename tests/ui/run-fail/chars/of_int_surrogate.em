-- サロゲートは Unicode のスカラー値ではないので、`Char.of_int` は `Panic` を起こし、捕まえなければ実行時エラーになる。
import Char

main : Unit -> <IO, Panic> Unit
main () = println (show (Char.to_int (Char.of_int 0xD800)))
