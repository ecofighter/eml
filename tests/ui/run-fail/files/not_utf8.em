-- Reading a file that is not UTF-8 stops the program with a runtime error.
main : Unit -> <IO> Unit
main () =
  let f = Fs.open "not_utf8.txt"
  let (f, text) = Fs.read_all f
  Fs.close f
  println text
