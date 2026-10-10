-- A polymorphic function projects and updates a record with a type parameter, used at two types.
data Pair a =
  | Pair { first : a, second : a }
  deriving (Show)

swap : Pair a -> Pair a
swap p =
  let first = p.first
  { p | first = p.second, second = first }

main : Unit -> <IO> Unit
main () =
  println (show (swap (Pair { first = 1, second = 2 })))
  println (show (swap (Pair { first = "a", second = "b" })))
  println (show (swap (Pair { first = 1, second = 2 })).first)
