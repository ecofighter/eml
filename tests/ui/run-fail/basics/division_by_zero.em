-- Division by zero stops the program and reports where the division is.
divide : Int -> Int -> Int
divide a b = a / b

main : Unit -> <IO> Unit
main () = println (show (divide 1 0))
