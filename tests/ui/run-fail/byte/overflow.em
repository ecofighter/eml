-- `Byte` の `+` は結果が 255 を超えると `Panic` を起こし、捕まえなければ実行時エラーになる。
main : Unit -> <IO, Panic> Unit
main () = println (show (255u8 + 1u8))
