-- List literals: trailing commas, multi-line literals, nesting, left-to-right evaluation, and `[]` in a module that
-- defines its own `Nil`.
data Stack =
  | Nil
  | Push Int Stack

noisy : Int -> <IO> Int
noisy n =
  println ("eval " ++ show n)
  n

sum : List Int -> Int
sum [] = 0
sum (x :: rest) = x + sum rest

main : Unit -> <IO> Unit
main () =
  let xs = [
    1,
    2,
  ]
  println (show xs)
  println (show [[1], [], [2, 3]])
  println (show ([] : List Int))
  println (show [noisy 1, noisy 2, noisy 3])
  println (show (sum [1, 2, 3, 4]))
  println (show (1 :: 2 :: []))
  println (show ([1] == [1]))
  match ([] : List Int) with
    | [] -> println "empty"
    | _ -> println "other"
