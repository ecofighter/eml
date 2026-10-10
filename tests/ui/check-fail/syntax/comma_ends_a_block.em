-- A comma in the middle of a line ends the lambda body, so the next line at the body's column is reported at the comma.
effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () =
  handle (fn () ->
    let x = ask (), 2
    println "a"
    println (show x)) with
    | ask () k -> k 1
    | return v -> v
