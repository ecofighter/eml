-- E3003: every branch must consume a file; the `else` branch forgets to close it.
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "input.txt"
  if True then FileIn.close f else println "kept"
