-- E1017: a lambda binds `x` twice in its parameters.
main : Unit -> <IO> Unit
main () =
  let f = fn x x -> x
  println (show (f 1 2))
