-- E2002: the body of a top-level value is checked with the empty row, so it cannot perform `Panic`.
width : Int
width = 80 * 2

main : Unit -> <IO> Unit
main () = println (show width)
