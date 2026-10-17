-- The callback that the performer passes to the function in the result of `each` reaches the clause, so `each` fixes
-- it to be callable any number of times, like an argument of an operation. The clause may then call it twice.
effect Iter where
  each : Unit -> ((Int -> <IO> Unit) -> <IO> Unit, Int)

main : Unit -> <IO> Unit
main () =
  handle (match each () with
    | (run, _) -> run (fn i -> println "\{i}")) with
    | ctl each () k ->
        k (fn f ->
          f 1
          f 2, 0)
