-- A `multi` continuation is resumed twice with different states, and each resumption keeps its own state.
effect Choose where
  multi choose : Unit -> Bool

pick : Unit -> <Choose> String
pick () = if choose () then "a" else "b"

main : Unit -> <IO> Unit
main () =
  let all =
    handle pick () from "" with
      | choose () k log -> k True (log ++ "T") ++ "|" ++ k False (log ++ "F")
      | return x log -> log ++ ":" ++ x
  println all
