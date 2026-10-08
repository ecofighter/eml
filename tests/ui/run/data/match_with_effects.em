-- A non-tail `match` whose arms perform a multi-shot operation. The string saved across the operation, the
-- field bound by the arm and the parameters of the arm's merge block survive every resumption.
effect Choice where
  multi choose : Unit -> Bool

data Option a =
  | None
  | Some a

pick : Option String -> <Choice> String
pick o =
  let prefix = "["
  let picked = match o with
    | Some s -> if choose () then s else s ++ "?"
    | None -> if choose () then "yes" else "no"
  prefix ++ picked ++ "]"

both : Option String -> String
both o =
  handle pick o with
    | choose () k -> k True ++ " " ++ k False

main : Unit -> <IO> Unit
main () =
  println (both (Some "a"))
  println (both None)
