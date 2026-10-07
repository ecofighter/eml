-- Reading a directory stops the program with a runtime error. Opening it succeeds on Unix; reading it fails.
main : Unit -> <IO> Unit
main () =
  let f = Fs.open "."
  let (f, text) = Fs.read_all f
  Fs.close f
  println text
