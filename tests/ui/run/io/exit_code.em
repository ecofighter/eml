-- `exit` ends the program with its code, after the output so far.
main : Unit -> <IO> Unit
main () =
  println "before"
  exit 3
  println "after"
