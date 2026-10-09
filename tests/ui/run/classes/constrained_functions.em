-- Constraints pass through generic functions and superclasses, and are resolved where the type
-- becomes known.
class Describe a where
  describe : a -> String

class Describe a => Labeled a where
  label : a -> String

data Color =
  | Red
  | Green

instance Describe Color where
  describe Red = "red"
  describe Green = "green"

instance Labeled Color where
  label c = "color " ++ describe c

data Box a = | Box a

instance Describe a => Describe (Box a) where
  describe (Box x) = "box of " ++ describe x

twice : Describe a => a -> String
twice x = describe x ++ " and " ++ describe x

outer : Describe a => a -> String
outer x = "[" ++ twice x ++ "]"

full : Labeled a => a -> String
full x = label x ++ " / " ++ describe x

main : Unit -> <IO> Unit
main () =
  println (outer Red)
  println (outer (Box (Box Green)))
  println (full Green)
