kind : Char -> Int
kind c = match c with
  | 'a' -> 1
  | '\n' -> 2
  | '\u{1F600}' -> 3
  | _ -> 0

main : Unit -> <IO, Panic> Unit
main () =
  println (show (kind 'a'))
  println (show (kind '\n'))
  println (show (kind '😀'))
  println (show (kind 'z'))
