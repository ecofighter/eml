-- A method with its own constrained type variable, and an instance whose head variable has the
-- same name as a method variable.
class Describe a where
  describe : a -> String

instance Describe Int where
  describe n = show_int n

data Box a = | Box a

class Fold f where
  fold : f -> b -> (b -> Int -> b) -> b
  tagged : Describe b => f -> b -> String

instance Fold (Box b) where
  fold (Box _) acc step = step acc 1
  tagged (Box _) x = "box:" ++ describe x

main : Unit -> <IO> Unit
main () =
  println (show_int (fold (Box "x") 41 (fn acc n -> acc + n)))
  println (tagged (Box True) 7)
