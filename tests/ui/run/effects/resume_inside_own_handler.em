-- A continuation is resumed inside the extent of its own handler; each run of the handler keeps its own state.
data Re = | Re (Re -> Int -> Int)

effect G where
  multi grab : Unit -> Re
  get : Unit -> Int
  put : Int -> Unit

body : Unit -> <G> Int
body () =
  let r = grab ()
  let n = get ()
  if n == 0 then
    put 1
    match r with
      | Re k ->
        let inner = k r 100
        inner + get ()
  else n

run : Unit -> Int
run () =
  handle body () from 0 with
    | grab () k st -> k (Re k) st
    | get () k st -> k st st
    | put n k _ -> k () n
    | return x _ -> x

main : Unit -> <IO> Unit
main () = println (show (run ()))
