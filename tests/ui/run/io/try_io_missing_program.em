-- `Proc.run` returns an `Int`; when it fails, its result slot holds the dummy object until the yield check sends the
-- abort on, and `try_io` catches it.
main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Proc.run "eml-no-such-program" []) with
    | Ok code -> println "exit \{code}"
    | Err e -> println "\{show e.kind}: \{e}"
