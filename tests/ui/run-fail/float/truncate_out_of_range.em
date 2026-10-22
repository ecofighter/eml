-- `Int` の範囲を外れた `Float` を `Int` に変えると `Panic` で、捕まえなければ実行時エラーになる。
import Float

main : Unit -> <IO, Panic> Unit
main () = println (show (Float.truncate 1e19))
