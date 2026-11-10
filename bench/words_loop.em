-- 3万語の文字列の語を数える。語ごとに、呼んだ側が持ち続ける文字列を、展開されない再帰の関数2つへ渡す。
-- 呼ばれる側が文字列を読むだけなら、渡すたびのRCが`rc_increments`に出ない。
import Byte
import String

grow : Int -> String -> <Panic> String
grow n acc = if n == 0 then acc else grow (n - 1) (acc ++ " ab")

skip_spaces : String -> Int -> <Panic> Int
skip_spaces s i =
  if i >= String.byte_length s then i
  else if String.byte_at i s == ' 'u8 then skip_spaces s (i + 1) else i

skip_word : String -> Int -> <Panic> Int
skip_word s i =
  if i >= String.byte_length s then i
  else if String.byte_at i s == ' 'u8 then i else skip_word s (i + 1)

count : String -> Int -> Int -> <Panic> Int
count s i acc =
  if i >= String.byte_length s then acc
  else count s (skip_word s (skip_spaces s i)) (acc + 1)

main : Unit -> <IO, Panic> Unit
main () =
  let s = grow 30000 ""
  println (show (count s 0 0))
