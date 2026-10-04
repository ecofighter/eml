-- E1002: an annotation in the body names a type variable that is not in the signature.
id : a -> a
id x = (x : b)

main : Unit -> <IO> Unit
main () = ()
