-- Tuples are built, passed around and taken apart by `let`, `match`, lambda and equation parameters,
-- including nested tuples and tuples inside an `Option`.
data Option a =
  | None
  | Some a

swap : (Int, String) -> (String, Int)
swap (n, s) = (s, n)

sum_pair : (Int, Int) -> Int
sum_pair p =
  let (a, b) = p
  a + b

lookup : String -> Option (String, Int)
lookup key = if key == "two" then Some ("two", 2) else None

describe : Option (String, Int) -> String
describe o = match o with
  | Some (name, n) -> name ++ "=" ++ show_int n
  | None -> "missing"

nested : ((Int, Int), String) -> String
nested ((a, b), label) = label ++ show_int (a * b)

main : Unit -> <IO> Unit
main () =
  let (s, n) = swap (1, "one")
  println (s ++ " " ++ show_int n)
  println (show_int (sum_pair (3, 4)))
  println (describe (lookup "two"))
  println (describe (lookup "three"))
  println (nested ((6, 7), "product "))
  let add = fn (x, y) -> x + y
  println (show_int (add (10, 20)))
