-- 100000 nested calls must not overflow the Rust stack: frames live on the heap.
count : Int -> <Panic> Int
count n = if n == 0 then 0 else 1 + count (n - 1)

main : Unit -> <IO, Panic> Unit
main () = println (show (count 100000))
