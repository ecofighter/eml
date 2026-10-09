-- A user class with a default method, an instance that keeps it and one that overrides it.
class Describe a where
  describe : a -> String
  loud : a -> String
  loud x = describe x ++ "!"

data Color =
  | Red
  | Green

data Size =
  | Small
  | Large

instance Describe Color where
  describe Red = "red"
  describe Green = "green"

instance Describe Size where
  describe Small = "small"
  describe Large = "large"
  loud _ = "SIZE"

main : Unit -> <IO> Unit
main () =
  println (describe Red)
  println (loud Green)
  println (describe Large)
  println (loud Small)
