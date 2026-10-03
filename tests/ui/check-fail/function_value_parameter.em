-- E0004: calling a function-typed parameter needs function values (stage 2).
apply : (Int -> Int) -> Int -> Int
apply f x = f x

main : Unit -> <IO> Unit
main () = ()
