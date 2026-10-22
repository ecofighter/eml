-- Reading a directory stops the program with a runtime error. Opening it succeeds on Unix; reading it fails.
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "."
  let (text, f) = FileIn.read_all f
  FileIn.close f
  println text
