-- `twice` makes two masked calls in a row, and each callback yields to the `multi` handler of `Choice`. The second masked
-- call is a yield point inside the continuation of the first one. The callbacks' `get` skips the `State Int` handler
-- `twice` runs under and reaches the outer one.
effect Choice where
  multi choose : Unit -> Bool

effect State s where
  get : Unit -> s
  put : s -> Unit

twice : (Unit -> <e> Int) -> (Unit -> <e> Int) -> <State Int, Panic | e> Int
twice a b =
  let x = a ()
  let y = b ()
  x + y + get ()

flip : Unit -> <Choice, State Int> Int
flip () = if choose () then get () else 0

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle (handle (handle twice flip flip from 100 with
                            | ctl get () k st -> k st st
                            | ctl put n k _ -> k () n
                            | return x _ -> x) from 1 with
                    | ctl get () k st -> k st st
                    | ctl put n k _ -> k () n
                    | return x _ -> show x) with
            | ctl choose () k -> k True ++ " " ++ k False
  println r
