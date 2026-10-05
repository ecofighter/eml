-- The scrutinee is used again inside an arm and after the match, so the arm unpacks a shared value: it copies
-- the fields and keeps the cell.
data Option a =
  | None
  | Some a

show : Option String -> String
show o = match o with
  | Some s -> "Some " ++ s
  | None -> "None"

label : Option String -> String
label o =
  let first = match o with
    | Some s -> s ++ " / " ++ show o
    | None -> "none"
  first ++ " / " ++ show o

main : Unit -> <IO> Unit
main () =
  let o = Some "a"
  println (label o)
  println (label o)
  println (label None)
