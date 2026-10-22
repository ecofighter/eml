-- String の文字の読みと走査 (docs/spec/stdlib.md の「`String`」)。位置は UTF-8 のバイトの位置である
import Iter
import List
import String

grow : Int -> String -> <Panic> String
grow n acc = if n == 0 then acc else grow (n - 1) (String.push 'é' acc)

main : Unit -> <IO, Panic> Unit
main () =
  println (show (String.char_at 1 "a日b"))
  println (show (String.next 1 "a日b"))
  println (show (String.fold (fn n _ -> n + 1) 0 "a日b"))
  String.each (fn c -> println (show c)) "a日"
  println (show (Iter.to_list (Iter.take 2 (String.chars "hello"))))
  println (show (String.to_chars "a日b"))
  println (String.of_chars ['h', 'é', '日'])
  println (String.of_char '日')
  println (grow 3 "")
  println (show (try_panic (fn () -> String.char_at 2 "a日b")))
  println (show (try_panic (fn () -> String.char_at 4 "a日")))
