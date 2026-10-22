-- Reading a file that is not UTF-8 stops the program with a runtime error.
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "not_utf8.txt"
  let (text, f) = FileIn.read_all f
  FileIn.close f
  println text
