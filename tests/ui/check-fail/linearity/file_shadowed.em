-- E3003: shadowing a file that was not consumed leaves it unconsumed.
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "a.txt"
  let f = FileIn.open "b.txt"
  FileIn.close f
