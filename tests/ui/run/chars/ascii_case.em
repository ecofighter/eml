-- ASCII の大文字と小文字の変換。ASCII 以外と英字でない文字はそのままである。
import Char

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Char.to_ascii_upper 'a'))
  println (show (Char.to_ascii_upper 'z'))
  println (show (Char.to_ascii_upper 'M'))
  println (show (Char.to_ascii_upper '1'))
  println (show (Char.to_ascii_upper 'é'))
  println (show (Char.to_ascii_lower 'A'))
  println (show (Char.to_ascii_lower 'Z'))
  println (show (Char.to_ascii_lower 'm'))
  println (show (Char.to_ascii_lower '@'))
  println (show (Char.to_ascii_lower 'É'))
