-- E2007: `resume` passes a state exactly when its continuation comes from a handler with a state.
effect Ask where
  ask : Unit -> Int

missing : Unit -> Int
missing () =
  handle ask () from 0 with
    | ask () k st -> resume k st
    | return x st -> x

extra : Unit -> Int
extra () =
  handle ask () with
    | ask () k -> resume k 1 2

through : Unit -> Int
through () =
  handle ask () from 0 with
    | ask () k st ->
        let again = fn c -> resume c 1
        again k
    | return x _ -> x
