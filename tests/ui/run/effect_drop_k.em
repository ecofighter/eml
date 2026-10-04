-- `drop k` discards a continuation. The frames it holds are freed with the values they saved. `drop` also
-- discards other values.
effect Choose where
  choose : String -> Bool

pick : Unit -> <Choose> String
pick () =
  let first = "apple"
  let second = "banana"
  if choose "which" then first else second

main : Unit -> <IO> Unit
main () =
  let taken =
    handle pick () with
      | choose question k ->
          drop question
          resume k True
  println taken
  let dropped =
    handle pick () with
      | choose question k ->
          drop k
          "dropped " ++ question
  println dropped
