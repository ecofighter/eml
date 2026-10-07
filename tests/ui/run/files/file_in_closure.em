-- A lambda that captures a file is linear and can be called once.
main : Unit -> <IO> Unit
main () =
  let f = Fs.open "input.txt"
  let finish = fn () -> Fs.close f
  finish ()
  println "closed"
