-- Lambda arguments are checked after the other arguments, so a section can read the fields of the list's elements.
data Person =
  | Person { name : String, age : Int }

map : (a -> b) -> List a -> List b
map f xs = match xs with
  | [] -> []
  | x :: rest -> f x :: map f rest

main : Unit -> <IO> Unit
main () =
  let people = [Person { name = "ann", age = 30 }, Person { name = "bob", age = 41 }]
  println (show (map (.name) people))
  println (show (people |> map (.age)))
  println (show (map (fn p -> p.name ++ "!") people))
  println (show (map (+ 1) (map (.age) people)))
