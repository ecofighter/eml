-- E3003: a `FileOut` that is never closed or dropped is an error.
import FileOut

main : Unit -> <IO> Unit
main () =
  let f = FileOut.create "out.txt"
  println "forgot"
