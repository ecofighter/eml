-- E3004: a file cannot be discarded with `_`.
import FileIn

main : Unit -> <IO> Unit
main () =
  let (text, _) = FileIn.read_all (FileIn.open "input.txt")
  println text
