-- Recursion and arithmetic up to the edge of Int.
factorial : Int -> Int
factorial n = if n <= 1 then 1 else n * factorial (n - 1)

main : Unit -> <IO> Unit
main () =
  println (show (factorial 10))
  println (show (factorial 20))
