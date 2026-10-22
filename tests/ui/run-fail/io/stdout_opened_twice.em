import Stdout

main : Unit -> <IO, Panic> Unit
main () =
  let h = Stdout.open ()
  let g = Stdout.open ()
  Stdout.close g
  Stdout.close h
