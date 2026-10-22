-- A lambda that captures a file is linear and can be called once.
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "input.txt"
  let finish = fn () -> FileIn.close f
  finish ()
  println "closed"
