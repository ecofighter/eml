-- A non-tail `match` whose arms perform a multi-shot operation. The string saved across the operation, the
-- field bound by the arm and the arguments of the arm's join point survive every resumption.
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
    | choose () k -> resume k True ++ " " ++ resume k False

main : Unit -> <IO> Unit
main () =
  println (both (Some "a"))
  println (both None)
