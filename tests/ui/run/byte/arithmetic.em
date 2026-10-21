main : Unit -> <IO, Panic> Unit
main () =
  println (show (200u8 + 55u8))
  println (show (16u8 * 15u8))
  println (show (10u8 - 10u8))
  println (show (10u8 / 3u8))
  println (show (10u8 % 3u8))
  println (show (negate 0u8))
