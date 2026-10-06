-- A handler with a state and no `return` clause returns the value of its body and drops the state.
effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () =
  let n =
    handle ask () * 2 from "unused" with
      | ask () k s -> resume k 21 s
  println (show_int n)
