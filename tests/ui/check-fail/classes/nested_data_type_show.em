-- E2012: a nested data type cannot derive Show, because its instance would be needed at
-- infinitely many types.
data Nested a =
  | Flat a
  | Nest (Nested (a, a))
  deriving Show

main : Unit -> <IO> Unit
main () = println (show (Flat 1))
