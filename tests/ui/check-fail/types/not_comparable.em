-- E2006: `==` and `!=` compare only `Int`, `String` and `Bool`.
data Color =
  | Red
  | Green

same : Color -> Color -> Bool
same a b = a == b

pair : (Int, Int) -> Bool
pair p = p != (1, 2)

main : Unit -> <IO> Unit
main () = println "done"
