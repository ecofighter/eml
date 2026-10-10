-- A record constructor applied by position can be partially applied like any other constructor.
data Person =
  | Person { name : String, age : Int }
  deriving (Show)

map : (a -> b) -> List a -> List b
map f xs = match xs with
  | [] -> []
  | x :: rest -> f x :: map f rest

main : Unit -> <IO> Unit
main () =
  let people = map (Person "a") [1, 2, 3]
  println (show people)
