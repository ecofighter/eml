-- fixtures: only runnable through the test harness
-- fixture ids: 7..8
-- `String` と `Bytes` の引数は、借りた本体の先頭へのポインタだけを渡し、長さは別の `Int` で渡す。連結した文字列は
-- ヒープに、リテラルは凍った定数にある。

import Bytes

extern "C" pure symbol "eml_fixture_bytes_sum" string_sum : String -> Int -> Int
extern "C" pure symbol "eml_fixture_bytes_sum" bytes_sum : Bytes -> Int -> Int

main : Unit -> <IO> Unit
main () =
  let s = "a" ++ "bc"
  println (show (string_sum s 3))
  println (show (string_sum "é" 2))
  println (show (bytes_sum (Bytes.of_string "xyz") 2))
