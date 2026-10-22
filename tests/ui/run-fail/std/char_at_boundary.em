import String

main : Unit -> <IO, Panic> Unit
main () = println (show (String.char_at 1 "日本"))
