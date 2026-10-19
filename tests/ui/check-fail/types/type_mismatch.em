-- E2001 four times: an argument, an annotation, a condition, and a statement.
add : Int -> Int -> <Panic> Int
add a b = a + b

main : Unit -> <IO, Panic> Unit
main () =
  println (add 1 2)
  let x : Int = "one"
  if x then println "x"
  x
  ()
