-- Projections read a field by name or a tuple element by index, and field sections are functions.
data Person =
  | Person { name : String, age : Int }

-- The list comes first, so the element type is known when the section is checked.
map_list : List a -> (a -> b) -> List b
map_list xs f = match xs with
  | [] -> []
  | x :: rest -> f x :: map_list rest f

main : Unit -> <IO> Unit
main () =
  let p = Person { name = "ann", age = 30 }
  println p.name
  println (show (p.age + 1))
  let t = (1, ("two", 3))
  println (show t.0)
  println t.1.0
  println (show t.1.1)
  let people = [p, Person { name = "bob", age = 41 }]
  println (show (map_list people (.name)))
  println (show (map_list [(1, "a"), (2, "b")] (.0)))
