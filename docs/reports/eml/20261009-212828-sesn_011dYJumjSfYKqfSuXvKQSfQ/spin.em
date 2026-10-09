spin : Int -> Int
spin n = spin (n + 1)

main : Unit -> <IO> Unit
main () = println (show_int (spin 0))
