-- `multi` operations may be resumed any number of times. A call of `k` that is not the last use of the continuation
-- copies the captured frames, so each resumption runs the rest of the computation on its own. Dropping the
-- continuation or not using it releases the frames and the strings they saved.
effect Choice where
  multi choose : Unit -> Bool

pick : Unit -> <Choice> String
pick () =
  let prefix = "picked "
  let n = if choose () then 1 else 2
  prefix ++ show_int n

both : Unit -> String
both () =
  handle pick () with
    | choose () k -> k True ++ " and " ++ k False

first_only : Unit -> String
first_only () =
  handle pick () with
    | choose () k -> k True

dropped : Unit -> String
dropped () =
  handle pick () with
    | choose () k ->
        drop k
        "dropped"

unused : Unit -> String
unused () =
  handle pick () with
    | choose () k -> "unused"

counted : Unit -> String
counted () =
  handle pick () with
    | choose () k -> k False ++ "/" ++ k True
    | return s -> "<" ++ s ++ ">"

main : Unit -> <IO> Unit
main () =
  println (both ())
  println (first_only ())
  println (dropped ())
  println (unused ())
  println (counted ())
