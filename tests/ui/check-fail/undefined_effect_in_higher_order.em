-- E1002 only: the row of an undefined effect accepts any effect at each call site, so the calls from `loud` and
-- `quiet` do not constrain each other.
run : (Unit -> <Console> Unit) -> <Console> Unit
run f = f ()

loud : Unit -> <IO> Unit
loud () = run (fn () -> println "x")

quiet : Unit -> Unit
quiet () = run (fn () -> ())

main : Unit -> <IO> Unit
main () = ()
