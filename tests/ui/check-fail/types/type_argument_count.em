-- E1015 twice: a data type applied to too few and too many type arguments.
data Option a =
  | None
  | Some a

size : Option -> Int
size o = 0

pair : Option Int Int -> Int
pair o = 0

main : Unit -> <IO> Unit
main () = println "done"
