-- E2002: `main` performs a user-defined effect that no handler handles.
effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () = println (show_int (ask ()))
