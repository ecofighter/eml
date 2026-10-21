class_of : Byte -> String
class_of b = match b with
  | 34u8 -> "q"
  | 44u8 -> "c"
  | _ -> "o"

main : Unit -> <IO> Unit
main () =
  println (show 65u8)
  println "\{65u8}"
  println (show (0xffu8 == 255u8))
  println (show (10u8 < 9u8))
  println (show (compare 0u8 255u8))
  println (class_of 44u8)
  println (show [1u8, 2u8])
