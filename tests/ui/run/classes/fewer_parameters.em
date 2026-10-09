-- An instance method defined with fewer parameters than the method's arrows: all arguments are
-- evaluated first, then the definition is called and the result applied to the rest.
class Combine a where
  combine : a -> a -> String

data Color =
  | Red
  | Green

name : Color -> String
name Red = "red"
name Green = "green"

joined : Color -> Color -> String
joined a b = name a ++ "+" ++ name b

pick : Color -> <IO> Color
pick c =
  println ("pick " ++ name c)
  c

instance Combine Color where
  combine = joined

main : Unit -> <IO> Unit
main () = println (combine (pick Red) (pick Green))
