-- A lambda piped into a function is checked after the function, so the `println` inside it is reported against the function's pure parameter type.
each : (String -> Unit) -> Unit
each f = f "x"

main : Unit -> <IO> Unit
main () = (fn s -> println s) |> each
