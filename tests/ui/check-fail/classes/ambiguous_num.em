-- E2009: arithmetic whose operand type nothing decides. Literals do not default to `Int`.
f : Unit -> Unit
f () = let g = fn x -> x + x in ()

main : Unit -> <IO> Unit
main () = println "done"
