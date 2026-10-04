-- `&&` and `||` evaluate the right operand only when the left one does not decide the result.
noisy : Bool -> <IO> Bool
noisy b =
  println "evaluated"
  b

show_bool : Bool -> String
show_bool b = if b then "True" else "False"

main : Unit -> <IO> Unit
main () =
  println (show_bool (False && noisy True))
  println (show_bool (True || noisy False))
  println (show_bool (True && noisy False))
