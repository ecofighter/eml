-- A join point whose only jumps sat in a removed join body is itself removed.
-- The string bound before the call still lives across it up to the `if` that uses it.
noisy : String -> Bool -> <IO> Bool
noisy name b =
  println name
  b

main : Unit -> <IO> Unit
main () =
  let other = "other"
  let a = noisy "a" True
  if (a || True) && True then println "x" else println other
  println "end"
