-- E2002: a lambda performs an effect that its expected type does not allow.
each : (String -> Unit) -> Unit
each f = f "a"

main : Unit -> <IO> Unit
main () = each (fn s -> println s)
