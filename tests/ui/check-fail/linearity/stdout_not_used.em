-- E3003: a `Stdout` handle must be used
import Stdout

main : Unit -> <IO, Panic> Unit
main () =
  let out = Stdout.open ()
  println "x"
