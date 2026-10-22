-- E3006: a file kept alive across a `multi` operation would be closed again by each resumption.
import FileIn

effect Choice where
  multi choose : Unit -> Bool

pick : Unit -> <Choice, IO> Unit
pick () =
  let f = FileIn.open "input.txt"
  let b = choose ()
  FileIn.close f

main : Unit -> <IO> Unit
main () =
  handle pick () with
    | ctl choose () k -> k True
