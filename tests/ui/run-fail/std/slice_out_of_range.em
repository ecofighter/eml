import String

main : Unit -> <IO, Panic> Unit
main () = println (String.slice 1 9 "abc")
