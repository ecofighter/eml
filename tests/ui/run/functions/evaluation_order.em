-- A call applies each arrow as soon as its argument is evaluated, whatever the callee is, with or without parentheses;
-- a piped value is evaluated first.
f : Int -> <IO> (Int -> <IO> Int)
f a =
  println "f"
  fn b -> a + b

g : Unit -> <IO> Int
g () =
  println "g"
  1

w : Int -> <IO> (Int -> <IO> Int)
w = fn a ->
  println "w"
  fn b -> a + b

main : Unit -> <IO> Unit
main () =
  println (show_int ((f 1) (g ())))
  println (show_int (f 1 (g ())))
  println (show_int (w 1 (g ())))
  let k = f
  println (show_int (k 1 (g ())))
  println (show_int ((fn a -> f a) 1 (g ())))
  println (show_int (g () |> k 1))
