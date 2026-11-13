import Stderr

main : Unit -> <IO, Panic> Unit
main () =
  let e = Stderr.open ()
  let e = Stderr.write_line "to stderr" e
  Stderr.close e
  println "done"
