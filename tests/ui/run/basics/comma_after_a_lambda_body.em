-- A comma in the middle of a line ends the lambda body opened by `->`, as it does when the lambda is on one line.
apply : (Int -> Int) -> Int -> Int
apply f x = f x

main : Unit -> <IO> Unit
main () =
  let pair = (fn x ->
      let y = x + 1
      y * 2, 10)
  println (show (apply pair.0 pair.1))
