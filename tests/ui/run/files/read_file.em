-- Reads a file next to this test. The second `read_all` starts where the first stopped, so it reads nothing.
import Fs

main : Unit -> <IO> Unit
main () =
  let f = Fs.open "input.txt"
  let (first, f) = Fs.read_all f
  let (rest, f) = Fs.read_all f
  Fs.close f
  println first
  println ("[" ++ rest ++ "]")
