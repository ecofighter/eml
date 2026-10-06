-- A call applies each arrow as soon as its argument is evaluated, with or without parentheses.
f : Int -> <IO> (Int -> <IO> Int)
f a =
  println "f"
  fn b -> a + b

g : Unit -> <IO> Int
g () =
  println "g"
  1

main : Unit -> <IO> Unit
main () =
  println (show_int ((f 1) (g ())))
  println (show_int (f 1 (g ())))
