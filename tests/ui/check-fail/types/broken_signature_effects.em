-- A broken signature or a missing arrow does not add E2002 for the effects of the body.
f : Int -> Undefined
f a b = println "x"

g : Int -> Unit
g a b = println "x"

main : Unit -> <IO> Unit
main () = ()
