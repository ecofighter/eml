-- The continuation of an inner handler is kept in the state of an outer handler and resumed later.
data R =
  | Done Int
  | Paused (Int -> R)

effect Ask where
  multi ask : Unit -> Int

effect Keep where
  stash : R -> Unit
  fetch : Unit -> R

step : Unit -> R
step () =
  handle Done (ask () + 1) with
    | ask () k -> Paused k

body : Unit -> <Keep> Int
body () =
  stash (step ())
  match fetch () with
    | Paused k ->
      match k 41 with
        | Done n -> n
        | Paused _ -> 0
    | Done n -> n

main : Unit -> <IO> Unit
main () =
  let r =
    handle body () from Done 0 with
      | stash v k _ -> k () v
      | fetch () k st -> k st st
      | return x _ -> x
  println (show r)
