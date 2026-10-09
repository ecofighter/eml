loop : Int -> Int -> Int
loop n acc = if n == 0 then acc else loop (n - 1) (acc + n)

main : Unit -> <IO> Unit
main () = println (show_int (loop 10000000 0))
