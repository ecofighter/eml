-- E0004: qualified effect names, which come with modules in stage S2.
f : Unit -> <Log.Log> Unit
f () = ()

main : Unit -> <IO> Unit
main () = println "done"
