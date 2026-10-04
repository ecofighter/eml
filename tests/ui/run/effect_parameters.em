-- Effects with type parameters. A handler that returns a function implements a state, `Reader` is used with
-- `String`, a function polymorphic in the type argument runs under handlers of different types, and with nested
-- handlers of one effect the inner one handles the operations while its clause performs at the outer one.
effect State s where
  get : Unit -> s
  put : s -> Unit

effect Reader r where
  ask : Unit -> r

counter : Unit -> <State Int> Int
counter () =
  put (get () + 1)
  put (get () * 10)
  get ()

run_counter : Int -> Int
run_counter start =
  let run =
    handle counter () with
      | get () k -> fn s -> (resume k s) s
      | put n k -> fn _ -> (resume k ()) n
      | return x -> fn _ -> x
  run start

greeting : Unit -> <Reader String> String
greeting () = "hello " ++ ask ()

with_name : String -> String
with_name name =
  handle greeting () with
    | ask () k -> resume k name

both_ways : (r -> r -> String) -> <Reader r> String
both_ways f = f (ask ()) (ask ())

nested : Unit -> String
nested () =
  handle (handle greeting () with | ask () k -> resume k (show_int (ask ()))) with
    | ask () k -> resume k 7

main : Unit -> <IO> Unit
main () =
  println (show_int (run_counter 1))
  println (with_name "Ada")
  let ints =
    handle both_ways (fn a b -> show_int (a + b)) with
      | ask () k -> resume k 21
  println ints
  let strings =
    handle both_ways (fn a b -> a ++ b) with
      | ask () k -> resume k "ab"
  println strings
  println (nested ())
