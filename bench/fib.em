-- 呼び出しと `Int` の算術。
fib : Int -> Int
fib n = if n < 2 then n else fib (n - 1) + fib (n - 2)

main : Unit -> <IO> Unit
main () = println (show (fib 25))
