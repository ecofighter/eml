-- Deep handlers also handle the operations performed after `resume`. With nested handlers of one effect, the
-- inner one handles the operation. A clause runs outside its handler, so its operations go to the outer one. An
-- operation walks past the handlers of other effects.
effect Counter where
  tick : Unit -> Int

effect Log where
  log : String -> Unit

count_three : Unit -> <Counter, Panic> Int
count_three () =
  let a = tick ()
  let b = tick ()
  let c = tick ()
  a + b + c

inner_and_outer : Unit -> <Counter, Panic> Int
inner_and_outer () =
  let outer = tick ()
  let inner =
    handle tick () + tick () with
      | ctl tick () k -> k 100
  outer + inner

logged_tick : Unit -> <Counter, Log> Int
logged_tick () =
  log "before"
  tick ()

main : Unit -> <IO, Panic> Unit
main () =
  let total =
    handle count_three () with
      | ctl tick () k ->
          println "tick"
          k 1
  println (show total)
  let mixed =
    handle inner_and_outer () with
      | ctl tick () k -> k 1
  println (show mixed)
  let relayed =
    handle (handle tick () with | ctl tick () k -> k (tick () + 10)) with
      | ctl tick () k -> k 5
  println (show relayed)
  let walked =
    handle (handle logged_tick () with | ctl tick () k -> k 7) with
      | ctl log message k ->
          println ("log: " ++ message)
          k ()
  println (show walked)
