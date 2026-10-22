-- `Bytes.append_slice from to src dst` は `dst` の後ろに `src` の `from` から `to` の手前までを足す。
main : Unit -> <IO, Panic> Unit
main () =
  let src = Bytes.of_string "0123456789"
  let dst = Bytes.append_slice 2 5 src (Bytes.of_string "<")
  println (show (dst == Bytes.of_string "<234"))
  let again = Bytes.append_slice 7 10 src dst
  println (show (again == Bytes.of_string "<234789"))
  let none = Bytes.append_slice 4 4 src again
  println (show (Bytes.length none))
  println (show (Bytes.length src))
  println (show (try_panic (fn () -> Bytes.length (Bytes.append_slice 3 11 src (Bytes.empty ())))))
  println (show (try_panic (fn () -> Bytes.length (Bytes.append_slice 5 3 src (Bytes.empty ())))))
  println (show (Bytes.length src))
