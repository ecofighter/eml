-- E1017: one equation binds `x` twice in its parameters.
add : Int -> Int -> Int
add x x = x

main : Unit -> <IO> Unit
main () = println (show (add 1 2))
