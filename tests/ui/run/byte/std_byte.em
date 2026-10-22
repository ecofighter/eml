import Byte

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Byte.to_int 200u8))
  println (show (Byte.wrap (-1)))
  println (show (Byte.wrap 256))
  println (show (Byte.wrap 300))
  println (show (try_panic (fn () -> Byte.of_int 256)))
  println (show (try_panic (fn () -> Byte.of_int (-1))))
  println (show (try_panic (fn () -> Byte.of_int 7)))
