-- E1033: `extern` is only allowed in the standard library, and using the declaration adds no other error.
extern shout : String -> String

main : Unit -> <IO> Unit
main () = println (shout "hi")
