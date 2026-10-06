-- A lambda piped into a function is checked through the Prelude's `|>`, so the mismatch is reported on the function.
each : (String -> Unit) -> Unit
each f = f "x"

main : Unit -> <IO> Unit
main () = (fn s -> println s) |> each
