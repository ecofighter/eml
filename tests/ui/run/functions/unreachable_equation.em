-- E4005: an equation after a catch-all is a warning, and the program still runs.
classify : Int -> String
classify _ = "any"
classify 0 = "zero"

main : Unit -> <IO> Unit
main () = println (classify 0)
