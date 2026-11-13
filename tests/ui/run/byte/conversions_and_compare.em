import Byte

main : Unit -> <IO, Panic> Unit
main () =
  let a = Byte.of_int 7
  let b = Byte.of_int 9
  println (show (Byte.to_int a))
  println (show (b > a))
  println (show (a > b))
  println (show (True != False))
  println (show (True != True))
