-- E3002: a file is linear, so closing it twice is an error.
main : Unit -> <IO> Unit
main () =
  let f = Fs.open "input.txt"
  Fs.close f
  Fs.close f
