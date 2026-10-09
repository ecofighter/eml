-- `never` operations abort the handled body. The frames of the aborted part are freed with the values they saved,
-- including a continuation that a clause saved before a `never` operation aborted it.
effect Fail where
  never fail : String -> a

effect Ask where
  ask : Unit -> Int

check_positive : Int -> <Fail> Int
check_positive n =
  if n > 0 then n else fail "not positive"

describe : Int -> <Fail> String
describe n =
  let label = "value: "
  let checked = check_positive n
  label ++ show checked

safe : Int -> String
safe n =
  handle describe n with
    | fail message -> "error: " ++ message

checked : Int -> <Fail> Int
checked n = if n > 10 then fail "too big" else n

sum_two : Unit -> <Ask> Int
sum_two () = ask () + ask ()

ask_checked : Int -> <Fail> Int
ask_checked answer =
  handle sum_two () with
    | ask () k ->
        let n = checked answer
        k n

bounded : Int -> String
bounded answer =
  handle show (ask_checked answer) with
    | fail message -> "aborted: " ++ message

main : Unit -> <IO> Unit
main () =
  println (safe 3)
  println (safe 0)
  println (bounded 4)
  println (bounded 20)
