-- ラッパーが自分の中で `State Int` を handle しつつ利用者のコールバックを呼ぶ。コールバックの `State` は内側の
-- handler に取られず、外側の `State String` に届く。
effect State s where
  get : Unit -> s
  put : s -> Unit

counted : (Unit -> <e> a) -> <e> (a, Int)
counted action =
  handle tick action from 0 with
    | get () k st -> resume k st st
    | put n k _ -> resume k () n
    | return x st -> (x, st)

tick : (Unit -> <e> a) -> <State Int | e> a
tick action =
  put (get () + 1)
  action ()

user : Unit -> <State String> String
user () = get () ++ "!"

main : Unit -> <IO> Unit
main () =
  let (s, n) = handle counted user from "outer" with
                 | get () k st -> resume k st st
                 | put v k _ -> resume k () v
                 | return x _ -> x
  println s
  println (show_int n)
