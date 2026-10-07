-- A hundred thousand operations resumed in tail position do not grow the continuation. The values saved across
-- each operation, including one bound by a join point, are freed exactly once.
effect Ask where
  ask : Unit -> Int

sum_asks : Int -> Int -> String -> <Ask> String
sum_asks n acc prefix =
  if n == 0 then prefix ++ show_int acc
  else
    let bonus = if n % 2 == 0 then 1 else 0
    let x = ask ()
    sum_asks (n - 1) (acc + x + bonus) prefix

main : Unit -> <IO> Unit
main () =
  let result =
    handle sum_asks 100000 0 "total: " with
      | ask () k -> k 2
  println result
