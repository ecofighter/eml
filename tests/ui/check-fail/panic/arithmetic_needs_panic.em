-- E2002: `+` performs `Panic`, so a function that adds needs `Panic` in its row.
inc : Int -> Int
inc n = n + 1

main : Unit -> <IO, Panic> Unit
main () = println (show (inc 1))
