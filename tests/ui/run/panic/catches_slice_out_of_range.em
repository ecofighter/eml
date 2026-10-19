-- `try_panic` が文字列の範囲外の切り出しと添字、文字の境目でない切り出しを捕まえる。
main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> String.slice 1 9 "abc")))
  println (show (try_panic (fn () -> String.byte_at 5 "abc")))
  println (show (try_panic (fn () -> String.slice 0 1 "é")))
  println (show (try_panic (fn () -> String.slice 1 2 "abc")))
