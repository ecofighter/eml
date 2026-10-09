-- E0003: `=>` is a reserved symbol, so it cannot be defined as an operator.
(=>) : Int -> Int -> Int
a => b = a + b

main : Unit -> <IO> Unit
main () = ()
