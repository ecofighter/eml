-- A lambda that captures a file is linear and can be called once.
main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  let finish = fn () -> close f
  finish ()
  println "closed"
