-- E2009: a method reference whose type is never decided.
main : Unit -> <IO> Unit
main () =
  let render = show
  println "done"
