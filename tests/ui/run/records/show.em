-- Derived Show writes records with field names; a record needs no parentheses as an argument.
data P = | P { name : String, age : Int } deriving (Show, Eq, Ord)
data A = | A {} deriving (Show)

main : Unit -> <IO> Unit
main () =
  println (show (P { name = "a", age = -1 }))
  println (show (Some P { name = "b", age = 2 }))
  println (show (A {}))
  println (show (compare (P { name = "a", age = 2 }) (P { name = "a", age = 1 })))
