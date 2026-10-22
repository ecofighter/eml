-- 0 から 255 の外の `Int` は `Byte` に入らないので、`Byte.of_int` は `Panic` を起こし、捕まえなければ実行時エラーになる。
import Byte

main : Unit -> <IO, Panic> Unit
main () = println (show (Byte.to_int (Byte.of_int 256)))
