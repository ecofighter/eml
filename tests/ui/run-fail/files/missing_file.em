-- Opening a file that does not exist stops the program with a runtime error.
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "missing.txt"
  FileIn.close f
