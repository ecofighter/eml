import Fail (Fail(..))

check : Int -> <Fail String, Panic> (Int -> <Fail String, Panic> Int)
check = fn a -> if a == 0 then Fail.fail "zero" else fn b -> a + b

call2 : (Int -> <Fail String, Panic> (Int -> <Fail String, Panic> Int)) -> Int -> <Fail String, Panic> Int
call2 g n =
  let r = g n 1
  r + 1

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Fail.to_result (fn () -> call2 check 0)))
  println (show (Fail.to_result (fn () -> call2 check 5)))
