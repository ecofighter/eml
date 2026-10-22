-- `drop` releases a file like `close` does.
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "input.txt"
  drop f
  println "dropped"
