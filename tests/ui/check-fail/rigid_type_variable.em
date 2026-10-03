-- E2001: a signature type variable is rigid in the body.
first : a -> b -> a
first x y = y

main : Unit -> <IO> Unit
main () = ()
