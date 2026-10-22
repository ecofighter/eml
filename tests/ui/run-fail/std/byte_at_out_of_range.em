import String

main : Unit -> <IO, Panic> Unit
main () = println (show (String.byte_at 3 "abc"))
