-- E2002: `greet` calls `println` but its signature has no `IO`.
greet : String -> Unit
greet name = println name

main : Unit -> <IO> Unit
main () = greet "eml"
