-- E2010: a type with a linear field cannot have instances.
import FileIn

data Handle = | Handle FileIn.FileIn deriving Eq

main : Unit -> <IO> Unit
main () = ()
