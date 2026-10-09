-- Int arithmetic is checked: overflow stops the program.
main : Unit -> <IO> Unit
main () =
  println "before"
  println (show (9223372036854775807 + 1))
