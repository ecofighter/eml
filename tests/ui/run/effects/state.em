-- A handler keeps a counter as its state; `get` and `put` read and replace it, and `return` receives the last state.
effect Counter where
  get : Unit -> Int
  put : Int -> Unit

count : Unit -> <Counter> Int
count () =
  put (get () + 1)
  put (get () + 10)
  get ()

main : Unit -> <IO> Unit
main () =
  let (result, final) =
    handle count () from 5 with
      | get () k st -> k st st
      | put n k _ -> k () n
      | return x st -> (x, st)
  println (show result)
  println (show final)
