-- E3003: a function whose result variable has a class context may return a value, so its call does not end the path.
import FileIn

class Default a where
  default : Unit -> a

instance Default Int where
  default () = 0

mk : Default a => Unit -> a
mk () = default ()

pick : Unit -> <IO> Int
pick () =
  let f = FileIn.open "input.txt"
  mk ()

main : Unit -> <IO> Unit
main () = println "\{pick ()}"
