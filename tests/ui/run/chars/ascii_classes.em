-- ASCII の判定が、0 から 127 のうち期待する個数の文字で成り立つ。
count : (Char -> Bool) -> Int
count p =
  let go n = if n > 127 then 0 else (if p (Char.of_int n) then 1 else 0) + go (n + 1)
  match try_panic (fn () -> go 0) with
    | Ok k -> k
    | Err _ -> -1

main : Unit -> <IO, Panic> Unit
main () =
  println "digit \{count Char.is_ascii_digit}"
  println "upper \{count Char.is_ascii_upper}"
  println "lower \{count Char.is_ascii_lower}"
  println "alpha \{count Char.is_ascii_alpha}"
  println "alphanumeric \{count Char.is_ascii_alphanumeric}"
  println "space \{count Char.is_ascii_space}"
  println "ascii \{count Char.is_ascii}"
  println (show (Char.is_ascii 'é'))
