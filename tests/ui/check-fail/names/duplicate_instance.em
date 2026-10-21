-- E1035: a second instance for the same class and type, written by hand or derived.
class Size a where
  size : a -> Int

data Color =
  | Red
  | Green
  deriving (Eq, Eq)

instance Size Color where
  size _ = 1

instance Size Color where
  size _ = 2

main : Unit -> <IO> Unit
main () = ()
