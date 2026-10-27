import Std.Par

data R =
  | Done Int
  | Paused (Int -> <Panic> R)

effect Ask where
  multi ask : Unit -> Int

effect Note where
  note : Int -> Unit

loop : Int -> <Ask, Panic> Int
loop n = if n == 0 then ask () + 1 else loop (n - 1)

step : Int -> <Panic> R
step n =
  handle Done (loop n) with
    | ctl ask () k -> Paused k

pair : (Int -> <Panic> R) -> <Panic> Int
pair k =
  let (a, b) =
    Par.par (fn () -> 1) (fn () ->
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
