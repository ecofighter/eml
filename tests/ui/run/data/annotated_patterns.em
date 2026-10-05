-- Type annotations may appear in any pattern.
data Option a =
  | None
  | Some a

get : Option Int -> Int
get (Some (n : Int)) = n
get None = 0

main : Unit -> <IO> Unit
main () =
  let (x : Int) = get (Some 41)
  let label = match Some "done" with
    | Some (s : String) -> s
    | None -> "none"
  println (show_int (x + 1))
  println label
