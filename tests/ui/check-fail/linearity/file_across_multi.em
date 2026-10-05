-- E3006: a file kept alive across a `multi` operation would be closed again by each resumption.
effect Choice where
  multi choose : Unit -> Bool

pick : Unit -> <Choice, IO> Unit
pick () =
  let f = open "input.txt"
  let b = choose ()
  close f

main : Unit -> <IO> Unit
main () =
  handle pick () with
    | choose () k -> resume k True
