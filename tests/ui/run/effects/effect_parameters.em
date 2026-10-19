-- Effects with type parameters. A handler that returns a function implements a state, `Reader` is used with
-- `String`, a function polymorphic in the type argument runs under handlers of different types, and with nested
-- handlers of one effect the inner one handles the operations while its clause performs at the outer one.
effect State s where
  get : Unit -> s
  put : s -> Unit

effect Reader r where
  ask : Unit -> r

counter : Unit -> <State Int, Panic> Int
counter () =
  put (get () + 1)
  put (get () * 10)
  get ()

run_counter : Int -> <Panic> Int
run_counter start =
  let run =
    handle counter () with
      | ctl get () k -> fn s -> (k s) s
      | ctl put n k -> fn _ -> (k ()) n
      | return x -> fn _ -> x
  run start

greeting : Unit -> <Reader String> String
greeting () = "hello " ++ ask ()

with_name : String -> String
with_name name =
  handle greeting () with
    | ctl ask () k -> k name

both_ways : (r -> r -> <Panic> String) -> <Reader r, Panic> String
both_ways f = f (ask ()) (ask ())

nested : Unit -> String
nested () =
  handle (handle greeting () with | ctl ask () k -> k (show (ask ()))) with
    | ctl ask () k -> k 7

main : Unit -> <IO, Panic> Unit
main () =
  println (show (run_counter 1))
  println (with_name "Ada")
  let ints =
    handle both_ways (fn a b -> show (a + b)) with
      | ctl ask () k -> k 21
  println ints
  let strings =
    handle both_ways (fn a b -> a ++ b) with
      | ctl ask () k -> k "ab"
  println strings
  println (nested ())
