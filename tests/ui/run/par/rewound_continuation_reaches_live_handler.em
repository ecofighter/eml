-- 右の子が、状態を持つ内側の handler を越えて作った継続を再開し、`par` の後で親が自分の handler の操作を起こす。
-- `ask` の yield は `count` の handler の `prompt$` を抜けるので、継続は `rewind$` の閉包でその handler を作り直す
-- (docs/spec/core-ir.md の「handle と `prompt$`」)。`rewind$` が捕まえた退いた evidence から、親で走っている
-- evidence に `share` が届かないことを確かめる (docs/spec/object-model.md の「evidence の3つの物体」)。handle の
-- 本体は `pick` と `pick_body` で選ぶ閉包にして、handler の特殊化で handle が消えないようにする。
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

asker : Int -> <Note, Ask, Panic> Int
asker n =
  note n
  let a = ask ()
  note a
  a * 10

pick_body : Int -> <Panic> (Int -> <Note, Ask, Panic> Int)
pick_body n = if n == 0 then asker else pick_body (n - 1)

count : (Int -> <Note, Ask, Panic> Int) -> Int -> <Ask, Panic> Int
count f n =
  handle f n from 0 with
    | ctl note x k st -> k () (st + x)
    | return x st -> x + st

step : Int -> <Panic> R
step n =
  handle Done (count (pick_body 2) n) with
    | ctl ask () k -> Paused k

pair : (Int -> <Panic> R) -> <Panic> Int
pair k =
  let (a, b) =
    Par.par (fn () -> spin 40 + 1) (fn () ->
      match k 4 with
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
