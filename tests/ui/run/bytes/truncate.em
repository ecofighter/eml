-- `Bytes.truncate n b` は前の `n` バイトを返す。一意ならその場で長さだけを書き換え、共有していれば元は変わらない。
main : Unit -> <IO, Panic> Unit
main () =
  let unique = Bytes.append (Bytes.of_string "hello") (Bytes.of_string " world")
  let cut = Bytes.truncate 5 unique
  println (show (cut == Bytes.of_string "hello"))
  println (show (Bytes.length cut))
  let grown = Bytes.append cut (Bytes.of_string "!")
  println (show (grown == Bytes.of_string "hello!"))
  let shared = Bytes.of_string "abcdef"
  let part = Bytes.truncate 3 shared
  println (show (part == Bytes.of_string "abc"))
  println (show (Bytes.length shared))
  println (show (Bytes.length (Bytes.truncate 0 (Bytes.of_string "x"))))
  println (show (try_panic (fn () -> Bytes.length (Bytes.truncate 7 shared))))
  println (show (try_panic (fn () -> Bytes.length (Bytes.truncate (-1) shared))))
  println (show (Bytes.length shared))
