-- E3003: every branch must consume a file; the `else` branch forgets to close it.
main : Unit -> <IO> Unit
main () =
  let f = Fs.open "input.txt"
  if True then Fs.close f else println "kept"
