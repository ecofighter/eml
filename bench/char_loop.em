-- `byte_loop` と同じ 10 万バイトの文字列を、`char_at` で1文字ずつ読んで足し上げる。
import Char
import String

grow : Int -> String -> <Panic> String
grow n acc = if n == 0 then acc else grow (n - 1) (acc ++ "x")

sum_chars : String -> Int -> Int -> Int -> <Panic> Int
sum_chars s i n acc =
  if i == n then acc
  else
    let c = String.char_at i s
    sum_chars s (String.next i s) n (acc + Char.to_int c)

main : Unit -> <IO, Panic> Unit
main () =
  let s = grow 99999 "s"
  println (show (sum_chars s 0 (String.byte_length s) 0))
