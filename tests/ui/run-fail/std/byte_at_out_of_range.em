main : Unit -> <IO> Unit
main () = println (show (String.byte_at 3 "abc"))
