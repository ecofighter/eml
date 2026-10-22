-- E3002: a file is linear, so closing it twice is an error.
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "input.txt"
  FileIn.close f
  FileIn.close f
