-- E3002: a branch that ends in `exit` may leave a file open, but still must not close it twice.
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "input.txt"
  if True then FileIn.close f
  else
    FileIn.close f
    FileIn.close f
    exit 1
