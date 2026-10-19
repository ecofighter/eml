import Fail (Fail(..))

checked_div : Int -> Int -> <Fail String, Panic> Int
checked_div a b = if b == 0 then Fail.fail "div by zero" else a / b

nested : Int -> <Fail String, Panic> Int
nested n = checked_div 100 n + 1

pair_error : Int -> <Fail (Int, String)> Int
pair_error n = if n < 0 then Fail.fail (n, "negative") else n

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Fail.to_result (fn () -> checked_div 10 2)))
  println (show (Fail.to_result (fn () -> checked_div 10 0)))
  println (show (Fail.to_option (fn () -> checked_div 10 5)))
  println (show (Fail.to_option (fn () -> checked_div 10 0)))
  println (show (Fail.to_result (fn () -> nested 0)))
  println (show (Fail.to_result (fn () -> nested 4)))
  let outer = Fail.to_result (fn () ->
    let inner = Fail.to_result (fn () -> checked_div 1 0)
    match inner with
      | Ok v -> v
      | Err _ -> Fail.fail 7)
  println (show (outer : Result Int Int))
  println (show (Fail.to_result (fn () -> pair_error (0 - 3))))
