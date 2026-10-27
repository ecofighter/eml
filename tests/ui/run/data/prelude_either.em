-- docs/spec/declarations.md の「Prelude のクラス」: Either の導出した Show と Ord、線形な値を持つ Either の match による消費
import FileIn

consume : Either FileIn.FileIn Int -> <IO> Int
consume e = match e with
  | Left f ->
    FileIn.close f
    0
  | Right n -> n

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Left 1 : Either Int String))
  println (show (Right "x" : Either Int String))
  println (show ((Left 9 : Either Int String) < Right "a"))
  println (show (consume (Left (FileIn.open "prelude_either.em"))))
  println (show (consume (Right 5)))
