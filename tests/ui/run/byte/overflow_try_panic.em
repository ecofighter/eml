main : Unit -> <IO, Panic> Unit
main () =
  println (show (try_panic (fn () -> 0u8 - 1u8)))
  println (show (try_panic (fn () -> 255u8 + 1u8)))
  println (show (try_panic (fn () -> negate 1u8)))
  println (show (try_panic (fn () -> 1u8 / 0u8)))
