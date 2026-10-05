-- E3006: an outer `multi` operation copies the inner handler with its `return` clause, so the clause would close the
-- captured file once for each resumption.
effect Choice where
  multi choose : Unit -> Bool

effect Ask where
  ask : Unit -> Int

pick : Unit -> <Choice, IO> Unit
pick () =
  let f = open "input.txt"
  handle (if choose () then ask () else 0) with
    | ask () k -> resume k 1
    | return n -> close f

main : Unit -> <IO> Unit
main () =
  handle pick () with
    | choose () k -> resume k True
