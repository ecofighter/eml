-- E3002: the continuation of a `once` operation must be used exactly once, so resuming it twice is an error.
effect Ask where
  ask : Unit -> Int

twice : Unit -> Int
twice () =
  handle ask () with
    | ask () k -> resume k 1 + resume k 2
