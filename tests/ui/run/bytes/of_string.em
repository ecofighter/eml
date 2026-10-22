-- `Bytes.of_string` は中身を写す。リテラルの `String` からも、組み立てた `String` からも作れる。
main : Unit -> <IO, Panic> Unit
main () =
  let s = "héllo" ++ "!"
  let b = Bytes.of_string s
  println (show (Bytes.length b))
  println (show (Bytes.length (Bytes.of_string "")))
  println (show (Bytes.length (Bytes.empty ())))
  println (show (b == Bytes.of_string "héllo!"))
  println (show (Bytes.empty () == Bytes.of_string ""))
