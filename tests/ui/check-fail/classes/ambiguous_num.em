-- E2009: arithmetic whose operand type nothing decides. Nothing defaults the operand type, so no instance is chosen.
f : Unit -> Unit
f () = let g = fn x -> x + x in ()

main : Unit -> <IO> Unit
main () = println "done"
