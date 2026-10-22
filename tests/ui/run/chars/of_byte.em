-- `of_byte` は Latin-1 で読み、`to_byte` は 255 までを戻す。`utf8_length` は境界の4つの長さを返す。
import Char

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Char.of_byte 0xe9u8 == 'é'))
  println (show (Char.to_byte 'é' == 0xe9u8))
  println (show (Char.to_byte 'a'))
  println (show (Char.utf8_length 'a'))
  println (show (Char.utf8_length 'é'))
  println (show (Char.utf8_length 'あ'))
  println (show (Char.utf8_length '\u{1F600}'))
