-- Reads a file next to this test. The second `read_all` starts where the first stopped, so it reads nothing.
main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  let (f, first) = read_all f
  let (f, rest) = read_all f
  close f
  println first
  println ("[" ++ rest ++ "]")
