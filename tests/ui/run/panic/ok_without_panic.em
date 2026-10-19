-- 誤りが起きなければ、`try_panic` は結果を `Ok` で返す。
main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> 1 + 2)))
