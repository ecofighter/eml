-- The second `open` of a standard stream is a `Panic` that `try_panic` catches; the first handle stays usable
main : Unit -> <IO, Panic> Unit
main () =
  let out = Stdout.open ()
  let input = Stdin.open ()
  match try_panic (fn () -> Stdout.close (Stdout.open ())) with
    | Ok () -> println "opened twice"
    | Err message -> println "caught: \{message}"
  match try_panic (fn () -> Stdin.close (Stdin.open ())) with
    | Ok () -> println "opened twice"
    | Err message -> println "caught: \{message}"
  Stdin.close input
  let out = Stdout.write_line "still open" out
  Stdout.close out
