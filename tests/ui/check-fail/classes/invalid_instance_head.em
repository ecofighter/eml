-- E1039: an instance head must be a type constructor applied to distinct type variables, so a type
-- variable, a tuple and `Option Int` are not heads.
class Size a where
  size : a -> Int

data Option a =
  | None
  | Some a

instance Size a where
  size _ = 1

instance Size (Int, Int) where
  size _ = 2

instance Size (Option Int) where
  size _ = 3

main : Unit -> <IO> Unit
main () = ()
