-- E2010: a type with a linear field cannot have instances.
data Handle = | Handle Fs.File deriving Eq

main : Unit -> <IO> Unit
main () = ()
