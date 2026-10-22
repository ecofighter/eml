-- `'x'u8` は文字のコードポイントを値にする `Byte` のリテラルで、式にもパターンにも書ける。
import Bytes

is_quote : Byte -> Bool
is_quote b =
  match b with
    | '"'u8 -> True
    | _ -> False

main : Unit -> <IO, Panic> Unit
main () =
  println (show ('\n'u8 == 10u8))
  println (show ('a'u8 == 97u8))
  println (show ('\u{ff}'u8 == 255u8))
  println (show (is_quote '"'u8))
  println (show (is_quote 'a'u8))
  println (show (Bytes.get 0 (Bytes.of_string "A") == 'A'u8))
