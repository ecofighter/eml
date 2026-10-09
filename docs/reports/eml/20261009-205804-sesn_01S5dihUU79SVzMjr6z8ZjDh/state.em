effect Counter where
  get : Unit -> Int
  put : Int -> Unit

count : Int -> <Counter> Int
count n = if n == 0 then 0 else
  put (get () + 1)
  count (n - 1)

main : Unit -> <IO> Unit
main () =
  let (result, final) =
    handle count 1000000 from 0 with
      | get () k st -> k st st
      | put n k _ -> k () n
      | return x st -> (x, st)
  println (show_int final)
