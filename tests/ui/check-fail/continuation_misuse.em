-- E3001: the continuation of a `once` operation must be used exactly once. Resuming it twice, not using it,
-- discarding it with `_`, and capturing it in an operation clause, which may run more than once, are errors.
effect Ask where
  ask : Unit -> Int

twice : Unit -> Int
twice () =
  handle ask () with
    | ask () k -> resume k 1 + resume k 2

unused : Unit -> Int
unused () =
  handle ask () with
    | ask () k -> 0

discarded : Unit -> Int
discarded () =
  handle ask () with
    | ask () _ -> 0

captured : Unit -> <Ask> Int
captured () =
  handle ask () with
    | ask () k ->
        handle ask () with
          | ask () inner -> resume k (resume inner 1)
