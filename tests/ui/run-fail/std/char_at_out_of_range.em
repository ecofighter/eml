import String

main : Unit -> <IO, Panic> Unit
main () = println (show (String.char_at 6 "日本"))
