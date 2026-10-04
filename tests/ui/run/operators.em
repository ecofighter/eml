-- Precedence, prefix minus, truncating division, short-circuit logic, and pipes.
noisy : Bool -> <IO> Bool
noisy b =
  println "evaluated"
  b

show_bool : Bool -> String
show_bool b = if b then "True" else "False"

main : Unit -> <IO> Unit
main () =
  println (show_int (1 + 2 * 3 - 4))
  println (show_int (-2 * 3))
  println (show_int ((-7) / 2))
  println (show_int ((-7) % 2))
  println (show_bool (1 < 2 && 2 < 3))
  println (show_bool (False && noisy True))
  println (show_bool (True || noisy False))
  println (show_bool (True && noisy False))
  println (show_bool (not (1 == 2)))
  10 |> show_int |> println
