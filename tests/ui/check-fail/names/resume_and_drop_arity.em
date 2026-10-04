-- E1011: `resume` takes a continuation and a value, and `drop` takes one value. The continuation used only by the
-- wrong `resume` or `drop` is not reported again.
effect Ask where
  ask : Unit -> Int

one : Unit -> Int
one () =
  handle ask () with
    | ask () k -> resume k

two : Unit -> Int
two () =
  handle ask () with
    | ask () k ->
        drop k 1
        0
