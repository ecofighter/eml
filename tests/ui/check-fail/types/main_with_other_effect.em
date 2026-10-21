-- E2004: the row of `main` may hold only `IO` and `Panic`.
effect State s where
  get : Unit -> s

main : Unit -> <IO, State Int> Unit
main () = println (show (get ()))
