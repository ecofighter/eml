main : Unit -> <IO, Panic> Unit
main () =
  println (show (0xf0u8 &&& 0x3cu8))
  println (show (0xf0u8 ||| 0x0fu8))
  println (show (0xffu8 ^^^ 0x0fu8))
  println (show (complement 0u8))
  println (show (0x81u8 <<< 1))
  println (show (0x81u8 >>> 7))
  println (show (0x81u8 <<< 8))
  println (show (0x81u8 >>> (-1)))
  println (show (0x81u8 <<< (-7)))
  println (show (0xffu8 >>> 100))
  println (show (0xffu8 <<< (-100)))
  println (show (0xffu8 >>> (-8)))
