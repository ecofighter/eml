-- E3006: an outer `multi` operation copies the inner handler with its `return` clause, so the clause would close the
-- captured file once for each resumption.
import FileIn

effect Choice where
  multi choose : Unit -> Bool

effect Ask where
  ask : Unit -> Int

pick : Unit -> <Choice, IO> Unit
pick () =
  let f = FileIn.open "input.txt"
  handle (if choose () then ask () else 0) with
    | ctl ask () k -> k 1
    | return n -> FileIn.close f

main : Unit -> <IO> Unit
main () =
  handle pick () with
    | ctl choose () k -> k True
