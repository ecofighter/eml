-- E2001: an annotated pattern must have the type of the value it matches.
data Option a =
  | None
  | Some a

get : Option Int -> Int
get (Some (n : String)) = 1
get None = 0

main : Unit -> <IO> Unit
main () = ()
