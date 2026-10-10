-- An update builds a new value and leaves the original unchanged; field values are evaluated in source order.
data Person =
  | Person { name : String, age : Int, city : String }
  deriving (Show)

traced : String -> String -> <IO> String
traced label value =
  println label
  value

main : Unit -> <IO> Unit
main () =
  let p = Person { name = "ann", age = 30, city = "oslo" }
  let older = { p | age = p.age + 1 }
  println (show older)
  println (show p)
  let moved = { p | city = traced "city" "rome", name = traced "name" "anna" }
  println (show moved)
  let age = 5
  println (show { p | age })
