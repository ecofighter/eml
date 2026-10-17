-- E3002: a branch that ends in `exit` may leave a file open, but still must not close it twice.
main : Unit -> <IO> Unit
main () =
  let f = Fs.open "input.txt"
  if True then Fs.close f
  else
    Fs.close f
    Fs.close f
    exit 1
