x : Int
x = 1 + 2

main : Unit -> <IO> Unit
main () = println (show_int (x + x))
