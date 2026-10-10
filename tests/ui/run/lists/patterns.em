-- List patterns: `[]`, fixed-length patterns, nested patterns, `::`, and equations that choose by list shape.
data Stack =
  | Nil
  | Push Int Stack

describe : List Int -> String
describe [] = "empty"
describe [x] = "one: " ++ show x
describe [x, y] = "two: " ++ show (x + y)
describe (x :: rest) = "many, starting with " ++ show x

nested : List (List Int) -> Int
nested [[a], [], [b, c]] = a + b + c
nested _ = 0

-- A module with its own `Nil` still gets the Prelude's empty list from `[]`.
is_empty : List Int -> Bool
is_empty xs = match xs with
  | [] -> True
  | _ -> False

main : Unit -> <IO> Unit
main () =
  println (describe Prelude.Nil)
  println (describe (1 :: Prelude.Nil))
  println (describe (1 :: 2 :: Prelude.Nil))
  println (describe (1 :: 2 :: 3 :: Prelude.Nil))
  println (show (nested ((1 :: Prelude.Nil) :: Prelude.Nil :: (2 :: 3 :: Prelude.Nil) :: Prelude.Nil)))
  println (show (is_empty Prelude.Nil))
  match Push 1 Nil with
    | Push n Nil -> println (show n)
    | _ -> println "other"
