-- Reading a file that is not UTF-8 stops the program with a runtime error.
main : Unit -> <IO> Unit
main () =
  let f = open "not_utf8.txt"
  let (f, text) = read_all f
  close f
  println text
