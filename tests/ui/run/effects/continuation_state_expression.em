-- 状態のある handler で `k st (st + 1)` を続けると、再開のたびに状態が1ずつ進む。
effect Ask where
  ask : Unit -> Int

body : Unit -> <Ask> Int
body () =
  let a = ask ()
  let b = ask ()
  let c = ask ()
  a + b + c

main : Unit -> <IO> Unit
main () =
  let r =
    handle body () from 10 with
      | ask () k st -> k st (st + 1)
      | return x st -> show_int x ++ "/" ++ show_int st
  println r
