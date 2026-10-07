-- Reads a file next to this test. The second `read_all` starts where the first stopped, so it reads nothing.
main : Unit -> <IO> Unit
main () =
  let f = Fs.open "input.txt"
  let (f, first) = Fs.read_all f
  let (f, rest) = Fs.read_all f
  Fs.close f
  println first
  println ("[" ++ rest ++ "]")
