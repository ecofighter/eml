-- Reads a file next to this test. The second `read_all` starts where the first stopped, so it reads nothing.
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "input.txt"
  let (first, f) = FileIn.read_all f
  let (rest, f) = FileIn.read_all f
  FileIn.close f
  println first
  println ("[" ++ rest ++ "]")
