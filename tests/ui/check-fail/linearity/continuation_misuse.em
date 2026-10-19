-- E3002: the continuation of a `once` operation must be used exactly once, so resuming it twice is an error.
effect Ask where
  ask : Unit -> Int

twice : Unit -> <Panic> Int
twice () =
  handle ask () with
    | ctl ask () k -> k 1 + k 2
