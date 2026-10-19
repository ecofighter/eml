-- E1003: the second signature of `twice`.
twice : Int -> <Panic> Int
twice x = x + x

main : Unit -> <IO> Unit
main () = ()

twice : Int -> Int
