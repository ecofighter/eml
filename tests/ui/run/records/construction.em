-- Records are built with named fields in any order, with punning, and positionally; patterns take them apart by
-- name and may omit fields.
data Person =
  | Person { name : String, age : Int }
  deriving (Eq, Show)

describe : Person -> String
describe p = match p with
  | Person { age, name } -> "\{name} is \{age}"

main : Unit -> <IO> Unit
main () =
  let name = "ann"
  let a = Person { age = 30, name }
  let b = Person "bob" 41
  println (describe a)
  println (describe b)
  println (show (map_age a))
  println (show (a == Person { name = "ann", age = 30 }))

map_age : Person -> Int
map_age p = match p with
  | Person { age } -> age
