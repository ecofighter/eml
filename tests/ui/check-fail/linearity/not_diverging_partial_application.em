-- E3003: a call through a `let`-bound value does not end the path, even when the value is `die`.
import Fs

die : String -> <IO> a
die message =
  eprintln message
  exit 2

main : Unit -> <IO> Unit
main () =
  let f = Fs.open "input.txt"
  let d = die
  d "no"
