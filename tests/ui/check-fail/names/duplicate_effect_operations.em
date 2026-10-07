-- E1003: the operations of a duplicate effect cannot be used, and their uses add no other diagnostics.
effect Ask where
  ask : Unit -> Int

effect Ask where
  tell : Unit -> Int

main : Unit -> <IO> Unit
main () =
  let n = handle tell () with
    | tell () k -> k 2
  println (show_int n)
