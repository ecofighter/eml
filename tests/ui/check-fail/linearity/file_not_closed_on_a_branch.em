-- E3003: every branch must consume a file; the `else` branch forgets to close it.
main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  if True then close f else println "kept"
