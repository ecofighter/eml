-- E2006: a type without an instance, a type variable without a constraint (with a help to add
-- it), and a constraint that fails beyond the context of an instance (with a note naming the
-- constraint the reference needs).
data Color =
  | Red
  | Green

data Option a =
  | None
  | Some a
  deriving Show

same : a -> a -> Bool
same x y = x == y

main : Unit -> <IO> Unit
main () =
  println (show Red)
  println (show (same 1 2))
  println (show (Some Red))
