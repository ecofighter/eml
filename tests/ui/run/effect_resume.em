-- `once` operations resumed in tail position and not in tail position. After a resume that is not in tail
-- position, the clause runs the rest of its body with the value of the whole handler.
effect Ask where
  ask : String -> String

greet : Unit -> <Ask> String
greet () =
  let name = ask "name"
  let place = ask "place"
  "hello " ++ name ++ " from " ++ place

with_answers : Unit -> String
with_answers () =
  handle greet () with
    | ask key k -> resume k (key ++ "!")

logged : Unit -> <IO> String
logged () =
  handle greet () with
    | ask key k ->
        let answer = resume k key
        println ("asked " ++ key)
        answer
    | return result -> "[" ++ result ++ "]"

main : Unit -> <IO> Unit
main () =
  println (with_answers ())
  println (logged ())
