-- E3001: when the handled effect has a `multi` operation, the `return` clause may run more than once, so it cannot
-- capture a linear value such as the continuation of a `once` operation.
effect Ask where
  ask : Unit -> Int

effect Choice where
  multi choose : Unit -> Bool

pick : Unit -> <Choice> Int
pick () = if choose () then 1 else 2

captured : Unit -> Int
captured () =
  handle ask () with
    | ask () k ->
        handle pick () with
          | choose () c -> resume c True + resume c False
          | return n -> resume k n
