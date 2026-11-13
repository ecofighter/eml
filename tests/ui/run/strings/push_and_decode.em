import Bytes
import String

main : Unit -> <IO, Panic> Unit
main () =
  println (String.push 'c' "ab")
  println (String.decode (Bytes.of_string "xyz"))
