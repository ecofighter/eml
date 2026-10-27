-- 右の子が、状態を持つ handler の下で作った継続を再開し、`par` の後で親がその handler の操作を起こす。
-- 継続が捕まえた evidence から、親で走っている evidence に `share` が届かないことを確かめる
-- (docs/spec/object-model.md の「evidence の3つの物体」)。handle の本体は `pick` で選ぶ閉包にして、
-- handler の特殊化で handle が消えないようにする。
import Std.Par

data R =
  | Done Int
  | Paused (Int -> <Panic> R)

effect Ask where
  multi ask : Unit -> Int

effect Note where
  note : Int -> Unit

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

loop : Int -> <Ask, Panic> Int
loop n = if n == 0 then ask () + 1 else loop (n - 1)

step : Int -> <Panic> R
step n =
  handle Done (loop n) with
    | ctl ask () k -> Paused k

pair : (Int -> <Panic> R) -> <Panic> Int
pair k =
  let (a, b) =
    Par.par (fn () -> spin 40 + 1) (fn () ->
      match k 1 with
        | Done n -> n
        | Paused _ -> 0)
  a + b

body : Int -> <Note, Panic> Int
body n =
  if n == 0 then
    match step 3 with
      | Paused k ->
        let m = pair k
        note 5
        m
      | Done m -> m
  else
    note 1
    body (n - 1)

other : Int -> <Note, Panic> Int
other n =
  note n
  n

pick : Int -> <Panic> (Int -> <Note, Panic> Int)
pick n = if n == 0 then body else if n == 1 then other else pick (n - 2)

run_with : (Int -> <Note, Panic> Int) -> Int -> <Panic> Int
run_with f n =
  handle f n from 0 with
    | ctl note x k st -> k () (st + x)
    | return x st -> x + st

main : Unit -> <IO, Panic> Unit
main () =
  println (show (run_with (pick 2) 2))
