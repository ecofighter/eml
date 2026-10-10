-- Multi-line and raw strings are literal patterns.
classify : String -> String
classify s = match s with
  | r"a\b" -> "raw"
  | """
    two
    lines
    """ -> "multi"
  | _ -> "other"

main : Unit -> <IO> Unit
main () =
  println (classify "a\\b")
  println (classify "two\nlines")
  println (classify "x")
