-- `&&` and `||` in `if` conditions evaluate the right operand only when the left one does not decide the result,
-- and keep the order of effects. A string bound before the conditions lives across the calls in them.
noisy : String -> Bool -> <IO> Bool
noisy name b =
  println name
  b

main : Unit -> <IO> Unit
main () =
  let kept = "kept"
  if noisy "a" False && noisy "b" True then println "and: yes" else println "and: no"
  if noisy "c" True || noisy "d" False then println "or: yes" else println "or: no"
  if noisy "e" True && noisy "f" True then println "both: yes" else println "both: no"
  if noisy "k" True && noisy "l" False && noisy "m" True then println "three: yes" else println "three: no"
  let n = if noisy "g" False || noisy "h" True then 1 else 2
  println (show n)
  println kept
