-- E3006: the file is dropped after the arguments of `die` are evaluated, so it is held across the `choose` inside them.
import Fs

effect Choose where
  multi choose : Unit -> Bool

die : String -> <Choose, IO> a
die message =
  eprintln message
  exit 2

run : Unit -> <Choose, IO> Unit
run () =
  let f = Fs.open "input.txt"
  die (show (choose ()))

main : Unit -> <IO> Unit
main () = ()
