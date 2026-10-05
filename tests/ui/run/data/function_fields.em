-- Functions stored in constructor fields are taken out by `match` and called. A partially applied
-- constructor holds a closure that captures a local value.
data Op =
  | Unary (Int -> Int)
  | Binary (Int -> Int -> Int) Int

run : Op -> Int -> Int
run op x = match op with
  | Unary f -> f x
  | Binary g y -> g x y

main : Unit -> <IO> Unit
main () =
  let offset = 10
  let add = Binary (fn a -> fn b -> a + b + offset)
  println (show_int (run (Unary (fn n -> n * 2)) 21))
  println (show_int (run (add 5) 1))
