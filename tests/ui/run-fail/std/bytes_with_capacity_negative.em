import Bytes

main : Unit -> <IO, Panic> Unit
main () = println (show (Bytes.length (Bytes.with_capacity (-1))))
