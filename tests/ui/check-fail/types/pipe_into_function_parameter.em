-- A lambda piped into a function is checked against the parameter type, as in a direct call.
each : (String -> Unit) -> Unit
each f = f "x"

main : Unit -> <IO> Unit
main () = (fn s -> println s) |> each
