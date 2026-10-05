-- `==` and `!=` on `Int`, `String` and `Bool`: the type checker picks how to compare from the operand type.
yes_no : Bool -> String
yes_no b = if b then "yes" else "no"

main : Unit -> <IO> Unit
main () =
  let name = "eml"
  println (yes_no (1 + 1 == 2))
  println (yes_no (name == "eml"))
  println (yes_no (name != "eml"))
  println (yes_no ("a" ++ "b" == "ab"))
  println (yes_no (True == False))
  println (yes_no ((1 == 2) == False))
