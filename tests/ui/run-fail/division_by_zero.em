-- Division by zero stops the program and names the function.
divide : Int -> Int -> Int
divide a b = a / b

main : Unit -> <IO> Unit
main () = println (show_int (divide 1 0))
