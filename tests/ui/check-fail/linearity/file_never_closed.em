-- E3003: a file that is never closed or dropped is an error.
main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  println "forgot"
