-- An `if` without `else` whose `then` branch is a block.
fib : Int -> Int
fib n = if n < 2 then n else fib (n - 1) + fib (n - 2)

print_fibs : Int -> Int -> <IO> Unit
print_fibs i n =
  if i < n then
    println (show (fib i))
    print_fibs (i + 1) n

main : Unit -> <IO> Unit
main () = print_fibs 0 10
