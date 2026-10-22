-- `String.decode` の妥当でない UTF-8 は `Panic` なので、`try_panic` が位置つきの文言で捕まえる。
main : Unit -> <IO, Panic> Unit
main () =
  let bad = Bytes.append (Bytes.of_string "abc") (Bytes.slice 0 1 (Bytes.of_string "é"))
  println (show (try_panic (fn () -> String.decode bad)))
  println (show (try_panic (fn () -> String.decode (Bytes.of_string "ok"))))
