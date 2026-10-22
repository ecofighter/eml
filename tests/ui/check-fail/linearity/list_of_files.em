-- E3003: a list of files is linear, so it must not be left unused.
import FileIn

open_both : Unit -> <IO> Unit
open_both () =
  let files = [FileIn.open "a.txt", FileIn.open "b.txt"]
  println "opened"

main : Unit -> <IO> Unit
main () = open_both ()
