-- E1003: an operation and a function share the value namespace; the first definition wins.
effect Ask where
  ask : Unit -> Int

ask : Unit -> Int
ask () = 1

main : Unit -> <IO> Unit
main () =
  let n = handle ask () with
    | ask () k -> k 2
  println (show_int n)
