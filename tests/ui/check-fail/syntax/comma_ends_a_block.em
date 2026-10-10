-- A comma in the middle of a line ends the lambda body, so the next line at the body's column is reported at the comma.
effect Ask where
  ask : Unit -> Int

run : (Unit -> <Ask, IO> Unit) -> <IO> Unit
run body =
  handle body () with
    | ask () k -> k 1

main : Unit -> <IO> Unit
main () =
  run (fn () ->
    let x = ask (), 2
    println "a"
    println (show x))
