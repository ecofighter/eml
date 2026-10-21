-- スカラー値でない `Int` を `Char` にすると `Panic` で、`try_panic` が文言を `Err` で返す。
main : Unit -> <IO, Panic> Unit
main () =
  println (show (try_panic (fn () -> Char.to_int (Char.of_int 0x110000))))
  println (show (try_panic (fn () -> Char.to_int (Char.of_int (-1)))))
  println (show (try_panic (fn () -> Char.of_int 65)))
