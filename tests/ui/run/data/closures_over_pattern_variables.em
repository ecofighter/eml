-- A lambda made inside a `match` arm captures pattern variables and is called twice after the match.
data Pair a b =
  | Pair a b

adder : Pair Int String -> (Int -> String)
adder p = match p with
  | Pair n label -> fn m -> label ++ " " ++ show (n + m)

main : Unit -> <IO> Unit
main () =
  let f = adder (Pair 1 "sum")
  println (f 2)
  println (f 40)
