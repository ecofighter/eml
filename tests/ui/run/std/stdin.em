-- 標準入力 (docs/spec/stdlib.md)。入力は stdin.stdin で与え、最後の行は行の終わりを持たない
main : Unit -> <IO, Panic> Unit
main () =
  println (show (Stdin.read_line ()))
  println (show (Stdin.read_line ()))
  println (show (Stdin.read_all ()))
  println (show (Stdin.read_line ()))
  println (show (Stdin.read_all ()))
