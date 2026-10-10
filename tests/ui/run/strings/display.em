-- `display` is `show` except for `String`, which it returns as is. Instances may override it, and a polymorphic
-- caller uses the instance of the type it is called at.
data Color = | Red | Green deriving (Show)

data Name = | Name String

instance Show Name where
  show (Name s) = "Name " ++ show s
  display (Name s) = s

wrap : Show a => a -> String
wrap x = "<" ++ display x ++ ">"

main : Unit -> <IO> Unit
main () =
  println (display "hi")
  println (display 42)
  println (wrap "hi")
  println (wrap [1, 2])
  println (wrap (Some "x"))
  println (wrap Red)
  println (wrap (Name "bob"))
  println (show (Name "bob"))
