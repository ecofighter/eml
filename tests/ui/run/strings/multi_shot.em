-- A hole that performs a `multi` operation is resumed twice; each resumption builds its own string from the part
-- built before the hole.
effect Choice where
  multi choose : Unit -> Bool

main : Unit -> <IO> Unit
main () =
  let s = handle "x=\{1}\{choose ()},\{choose ()}" with
    | choose () k -> k True ++ " | " ++ k False
  println s
