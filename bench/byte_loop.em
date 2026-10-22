-- 10 万バイトの文字列を `byte_at` で先頭から足し上げる。1バイトの読みごとの RC が `rc_increments` に出る。
import Byte
import String

grow : Int -> String -> <Panic> String
grow n acc = if n == 0 then acc else grow (n - 1) (acc ++ "x")

sum_bytes : String -> Int -> Int -> Int -> <Panic> Int
sum_bytes s i n acc =
  if i == n then acc else sum_bytes s (i + 1) n (acc + Byte.to_int (String.byte_at i s))

main : Unit -> <IO, Panic> Unit
main () =
  let s = grow 99999 "s"
  println (show (sum_bytes s 0 (String.byte_length s) 0))
