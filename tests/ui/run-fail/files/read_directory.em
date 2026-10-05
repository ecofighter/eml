-- Reading a directory stops the program with a runtime error. Opening it succeeds on Unix; reading it fails.
main : Unit -> <IO> Unit
main () =
  let f = open "."
  let (f, text) = read_all f
  close f
  println text
