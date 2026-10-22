import Stdin

main : Unit -> <IO, Panic> Unit
main () =
  let h = Stdin.open ()
  let g = Stdin.open ()
  Stdin.close g
  Stdin.close h
