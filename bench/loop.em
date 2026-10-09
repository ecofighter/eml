-- 末尾再帰のループ。
loop : Int -> Int -> Int
loop i acc = if i == 0 then acc else loop (i - 1) (acc + i)

main : Unit -> <IO> Unit
main () = println (show (loop 500000 0))
