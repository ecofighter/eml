-- E1036: an instance must define every method that has no default.
class Size a where
  size : a -> Int
  double : a -> Int
  double x = size x * 2

data Color =
  | Red
  | Green

instance Size Color

main : Unit -> <IO> Unit
main () = ()
