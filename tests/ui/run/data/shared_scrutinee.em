-- 枝の中と `match` の後で、scrutinee をもう一度使う。scrutinee は枝の後も生きているので、枝は箱を残したまま、
-- 使うフィールドを複製する。
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
