-- Strings that are shadowed, used twice, and discarded must all be freed.
greet : String -> String
greet name = "Hello, " ++ name ++ "!"

main : Unit -> <IO> Unit
main () =
  let name = "eml"
  let message = greet name
  println message
  println name
  let name = name ++ name
  println name
  let _ = greet "unused"
  println (show_int (-42))
