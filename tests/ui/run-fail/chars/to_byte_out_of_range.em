-- 255 を超える文字は `Byte` に入らないので、`Char.to_byte` は `Panic` を起こし、捕まえなければ実行時エラーになる。
import Char

main : Unit -> <IO, Panic> Unit
main () = println (show (Char.to_byte 'あ'))
