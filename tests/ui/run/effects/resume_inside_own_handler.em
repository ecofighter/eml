-- A continuation is resumed inside the extent of its own handler; each run of the handler keeps its own state.
data Re = | Re (Re -> Int -> <Panic> Int)

effect G where
  multi grab : Unit -> Re
  get : Unit -> Int
  put : Int -> Unit

body : Unit -> <G, Panic> Int
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

run : Unit -> <Panic> Int
run () =
  handle body () from 0 with
    | ctl grab () k st -> k (Re k) st
    | ctl get () k st -> k st st
    | ctl put n k _ -> k () n
    | return x _ -> x

main : Unit -> <IO, Panic> Unit
main () = println (show (run ()))
