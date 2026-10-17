-- E3006: the branch that ends in `exit` drops the file just before the call, so it holds the file across `choose`.
effect Choose where
  multi choose : Unit -> Bool

run : Bool -> <Choose, IO> Unit
run c =
  let f = Fs.open "input.txt"
  if c then Fs.close f
  else
    let b = choose ()
    exit 2

main : Unit -> <IO> Unit
main () = ()
