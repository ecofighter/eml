-- E3006: `keep` holds its argument across a call of `action`. Given a file and an action that performs a `multi`
-- operation, the file would be closed again by each resumption.
import FileIn

effect Choice where
  multi choose : Unit -> Bool

keep : a -> (Unit -> <e> Unit) -> <e> a
keep x action =
  action ()
  x

chooser : Unit -> <Choice> Unit
chooser () =
  let b = choose ()
  ()

pick : Unit -> <Choice, IO> Unit
pick () =
  let f = FileIn.open "input.txt"
  let g = keep f chooser
  FileIn.close g

main : Unit -> <IO> Unit
main () =
  handle pick () with
    | ctl choose () k -> k True
