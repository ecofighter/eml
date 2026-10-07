-- E3003: shadowing a file that was not consumed leaves it unconsumed.
main : Unit -> <IO> Unit
main () =
  let f = Fs.open "a.txt"
  let f = Fs.open "b.txt"
  Fs.close f
