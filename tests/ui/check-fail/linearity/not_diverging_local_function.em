-- E3003: a local function is monomorphic, so calling one that ends in `exit` does not end the path.
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "input.txt"
  let stop n = exit n
  stop 1
