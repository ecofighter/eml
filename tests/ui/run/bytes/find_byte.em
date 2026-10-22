-- `Bytes.find_byte x b` は `x` が最初にある位置を返す。8 バイトより長い列では、語の境目の前後と末尾でも見つかる。
-- `Bytes.find` は先頭のバイトが合う位置ごとに残りを比べるので、先頭だけが合う位置を飛ばす。
main : Unit -> <IO, Panic> Unit
main () =
  let b = Bytes.of_string "the quick brown fox jumps over the lazy dog"
  println (show (Bytes.find_byte 116u8 b))
  println (show (Bytes.find_byte 107u8 b))
  println (show (Bytes.find_byte 120u8 b))
  println (show (Bytes.find_byte 103u8 b))
  println (show (Bytes.find_byte 0u8 b))
  println (show (Bytes.find_byte 255u8 (Bytes.make 20 255u8)))
  println (show (Bytes.find_byte 1u8 (Bytes.empty ())))
  println (show (Bytes.find (Bytes.of_string "the lazy") b))
  println (show (Bytes.find (Bytes.of_string "dog") b))
  println (show (Bytes.find (Bytes.of_string "dogs") b))
