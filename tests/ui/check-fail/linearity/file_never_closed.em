-- E3003: a file that is never closed or dropped is an error.
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "input.txt"
  println "forgot"
