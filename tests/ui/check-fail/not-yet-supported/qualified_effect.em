-- E0004: qualified effect names, which come with modules in stage S2.
-- The row is reported once; using another effect in the body adds no effect error.
f : Unit -> <Log.State Int> Unit
f () = println "x"

main : Unit -> <IO> Unit
main () = f ()
