-- E3003: a method whose result is the class variable may return a value, so its call does not end the path.
import FileIn

class Make a where
  make : Unit -> a

instance Make Int where
  make () = 0

pick : Unit -> <IO> Int
pick () =
  let f = FileIn.open "input.txt"
  make ()

main : Unit -> <IO> Unit
main () = println "\{pick ()}"
