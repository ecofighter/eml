-- The continuation of an inner handler is kept in the state of an outer handler and resumed later.
data R =
  | Done Int
  | Paused (Int -> <Panic> R)

effect Ask where
  multi ask : Unit -> Int

effect Keep where
  stash : R -> Unit
  fetch : Unit -> R

step : Unit -> <Panic> R
step () =
  handle Done (ask () + 1) with
    | ctl ask () k -> Paused k

body : Unit -> <Keep, Panic> Int
body () =
  stash (step ())
  match fetch () with
    | Paused k ->
      match k 41 with
        | Done n -> n
        | Paused _ -> 0
    | Done n -> n

main : Unit -> <IO, Panic> Unit
main () =
  let r =
    handle body () from Done 0 with
      | ctl stash v k _ -> k () v
      | ctl fetch () k st -> k st st
      | return x _ -> x
  println (show r)
