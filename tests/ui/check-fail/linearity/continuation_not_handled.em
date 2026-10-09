-- E3005: a clause of a `once` operation must resume or drop its continuation on every path.
effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () =
  let n =
    handle ask () with
      | ask () k -> 0
  println (show n)
