-- Derived Eq, Ord and Show for an enumeration, a parameterized type and a single-constructor type.
data Color =
  | Red
  | Green
  | Blue
  deriving (Eq, Ord, Show)

data Option a =
  | None
  | Some a
  deriving (Eq, Ord, Show)

data Point = | Point Int Int deriving (Eq, Ord, Show)

main : Unit -> <IO> Unit
main () =
  println (show (Red == Red) ++ " " ++ show (Red != Blue) ++ " " ++ show (Green < Blue))
  println (show (compare (Some 2) (Some 1)) ++ " " ++ show (None < Some 0))
  println (show (Point 1 2 < Point 1 3) ++ " " ++ show (Point 2 0 >= Point 1 9))
  println (show Blue ++ " " ++ show (Some (Some (-3))) ++ " " ++ show (Point 4 (-5)))
  println (show (Some "a\"b") ++ " " ++ show (None : Option Int))
