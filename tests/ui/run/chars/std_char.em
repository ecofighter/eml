import Char

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Char.to_int 'A'))
  println (show (Char.to_int '\u{10FFFF}'))
  println (show (Char.of_int 97))
  println (show (Char.of_int 0xE000 == '\u{E000}'))
