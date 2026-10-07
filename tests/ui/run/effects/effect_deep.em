-- Deep handlers also handle the operations performed after `resume`. With nested handlers of one effect, the
-- inner one handles the operation. A clause runs outside its handler, so its operations go to the outer one. An
-- operation walks past the handlers of other effects.
effect Counter where
  tick : Unit -> Int

effect Log where
  log : String -> Unit

count_three : Unit -> <Counter> Int
count_three () =
  let a = tick ()
  let b = tick ()
  let c = tick ()
  a + b + c

inner_and_outer : Unit -> <Counter> Int
inner_and_outer () =
  let outer = tick ()
  let inner =
    handle tick () + tick () with
      | tick () k -> k 100
  outer + inner

logged_tick : Unit -> <Counter, Log> Int
logged_tick () =
  log "before"
  tick ()

main : Unit -> <IO> Unit
main () =
  let total =
    handle count_three () with
      | tick () k ->
          println "tick"
          k 1
  println (show_int total)
  let mixed =
    handle inner_and_outer () with
      | tick () k -> k 1
  println (show_int mixed)
  let relayed =
    handle (handle tick () with | tick () k -> k (tick () + 10)) with
      | tick () k -> k 5
  println (show_int relayed)
  let walked =
    handle (handle logged_tick () with | tick () k -> k 7) with
      | log message k ->
          println ("log: " ++ message)
          k ()
  println (show_int walked)
