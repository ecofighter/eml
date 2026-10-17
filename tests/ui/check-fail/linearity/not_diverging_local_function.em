-- E3003: a local function is monomorphic, so calling one that ends in `exit` does not end the path.
main : Unit -> <IO> Unit
main () =
  let f = Fs.open "input.txt"
  let stop n = exit n
  stop 1
