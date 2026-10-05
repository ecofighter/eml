-- A tuple or a constructor built only to be matched is taken apart without being allocated.
data Option a =
  | None
  | Some a

add : Int -> Int -> Int
add a b = match (a, b) with
  | (x, y) -> x + y

unwrap : Int -> Int
unwrap x = match Some x with
  | Some y -> y
  | None -> 0

main : Unit -> <IO> Unit
main () =
  println (show_int (add 1 2))
  println (show_int (unwrap 7))
