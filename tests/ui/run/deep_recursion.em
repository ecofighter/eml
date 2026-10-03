-- 100000 nested calls must not overflow the Rust stack: frames live on the heap.
count : Int -> Int
count n = if n == 0 then 0 else 1 + count (n - 1)

main : Unit -> <IO> Unit
main () = println (show_int (count 100000))
