-- Each level resumes the continuation twice, so the first resumption copies the captured frames. Ten thousand
-- copies run and are released without leaks.
effect Choice where
  multi choose : Unit -> Bool

loop : Int -> Int -> <Choice> Int
loop n acc = if n == 0 then acc else if choose () then loop (n - 1) (acc + 1) else acc

main : Unit -> <IO> Unit
main () =
  let total =
    handle loop 10000 0 with
      | choose () k -> k False + k True
  println (show_int total)
