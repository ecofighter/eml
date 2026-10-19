-- `try_io` does not catch an arithmetic error.
main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> 1 / 0) with
    | Ok n -> println "ok \{n}"
    | Err e -> println "caught: \{e}"
