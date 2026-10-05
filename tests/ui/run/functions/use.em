-- `use` passes the rest of the block as the last argument, with and without a bound name.
effect Log where
  log : String -> Unit

with_log : (Unit -> <Log, IO> a) -> <IO> a
with_log body =
  handle body () with
    | log s k ->
        println ("log: " ++ s)
        resume k ()

twice : (Int -> <IO> Unit) -> <IO> Unit
twice k =
  k 1
  k 2

main : Unit -> <IO> Unit
main () =
  use with_log
  log "start"
  use n <- twice
  println (show_int n)
