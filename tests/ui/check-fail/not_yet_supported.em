-- E0004: constructs of later stages in HIR and in the type checker.
data Color = | Red | Green

add : Int -> Int -> Int
add a b = a + b

main : Unit -> <IO> Unit
main () =
  let f = fn x -> x
  let g = add
  let h = add 1
  println "done"
