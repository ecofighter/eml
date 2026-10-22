-- E3006: the branch that ends in `exit` drops the file just before the call, so it holds the file across `choose`.
import FileIn

effect Choose where
  multi choose : Unit -> Bool

run : Bool -> <Choose, IO> Unit
run c =
  let f = FileIn.open "input.txt"
  if c then FileIn.close f
  else
    let b = choose ()
    exit 2

main : Unit -> <IO> Unit
main () = ()
