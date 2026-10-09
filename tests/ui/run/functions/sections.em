-- Operator references and sections are lambdas; `(==)` compares by the type it is used at.
apply : (Int -> Int) -> Int -> Int
apply f x = f x

apply2 : (a -> a -> b) -> a -> a -> b
apply2 f x y = f x y

check : (String -> Bool) -> String -> String
check p s = if p s then "yes" else "no"

main : Unit -> <IO> Unit
main () =
  println (show (apply (+ 1) 41))
  println (show (apply (100 -) 1))
  println (show (apply2 (*) 6 7))
  println (show (apply (+ 2 * 3) 1))
  println (check (== "a") "a")
  println (check (!= "a") "a")
  println (if apply2 (==) 3 3 then "same" else "different")
  println (if apply2 (&&) True False then "both" else "not both")
