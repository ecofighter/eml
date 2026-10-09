-- 状態を持つ handler の下で数える再帰。`perform` と `k` の呼び出し。
effect Counter where
  get : Unit -> Int
  put : Int -> Unit

count : Int -> <Counter> Int
count n = if n == 0 then 0 else
  put (get () + 1)
  count (n - 1)

main : Unit -> <IO> Unit
main () =
  let (_, final) =
    handle count 100000 from 0 with
      | get () k st -> k st st
      | put n k _ -> k () n
      | return x st -> (x, st)
  println (show_int final)
