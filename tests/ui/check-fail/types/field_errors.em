-- E2013: a projection needs the type of its value to be known; E2014: the type has no such field.
data Person =
  | Person { name : String, age : Int }

data Shape =
  | Circle { radius : Int }
  | Square { radius : Int }

unknown : Unit -> Int
unknown () =
  let age_of = fn p -> p.age
  0

radius : Shape -> Int
radius s = s.radius

nick : Person -> String
nick p = p.nick

third : (Int, Int) -> Int
third t = t.2

main : Unit -> <IO> Unit
main () = println "done"
