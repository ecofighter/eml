-- Operations are values: they can be passed to functions, and a curried operation can be partially applied. A
-- partial application that is never called is freed.
effect Log where
  log : String -> String -> Unit
  note : String -> Unit

apply_twice : (String -> <e> Unit) -> <e> Unit
apply_twice f =
  f "first"
  f "second"

run : Unit -> <Log> Unit
run () =
  let unused = log "unused"
  apply_twice note
  apply_twice (log "info")

main : Unit -> <IO> Unit
main () =
  handle run () with
    | log level message k ->
        println (level ++ ": " ++ message)
        resume k ()
    | note message k ->
        println ("note: " ++ message)
        resume k ()
