-- E3006: a file kept alive across a `multi` operation would be closed again by each resumption.
import Fs

effect Choice where
  multi choose : Unit -> Bool

pick : Unit -> <Choice, IO> Unit
pick () =
  let f = Fs.open "input.txt"
  let b = choose ()
  Fs.close f

main : Unit -> <IO> Unit
main () =
  handle pick () with
    | ctl choose () k -> k True
