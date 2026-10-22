import Stdin

main : Unit -> <IO, Panic> Unit
main () =
  let (text, h) = Stdin.read_all (Stdin.open ())
  Stdin.close h
  println text
