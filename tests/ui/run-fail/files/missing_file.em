-- Opening a file that does not exist stops the program with a runtime error.
main : Unit -> <IO> Unit
main () =
  let f = open "missing.txt"
  close f
