-- `mask` を含む区間を持つ `multi` の継続を2回再開する。どちらの再開でも、コールバックの `name` は外側に届く。
effect Choice where
  multi choose : Unit -> Bool

effect Name where
  name : Unit -> String

named : (Unit -> <e> a) -> <e> a
named action =
  handle inner action with
    | ctl name () k -> k "inner"

inner : (Unit -> <e> a) -> <Name | e> a
inner action = action ()

pick : Unit -> <Choice, Name> String
pick () =
  let b = choose ()
  if b then name () else "no"

main : Unit -> <IO> Unit
main () =
  let r = handle (handle named pick with
                    | ctl name () k -> k "outer") with
            | ctl choose () k -> k True ++ " " ++ k False
  println r
