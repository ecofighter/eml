-- E1041: a constraint names a type, not a class.
twice : Int a => a -> a
twice x = x

main : Unit -> <IO> Unit
main () = ()
