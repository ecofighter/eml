-- 利用者の `panic` の文言を、`try_panic` がそのまま `Err` で返す。
check : Int -> <Panic> Int
check n = if n > 2 then panic "boom" else n

main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> check 5)))
