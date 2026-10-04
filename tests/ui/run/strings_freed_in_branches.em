-- Strings chosen by `if`, passed to an unused parameter, and discarded must all be freed.
twice : String -> String
twice s = s ++ s

ignore : String -> Int
ignore s = 1

pick : Bool -> String -> String
pick b s =
  let t = if b then s else "none"
  t ++ s

main : Unit -> <IO> Unit
main () =
  let s = "x"
  let s = s ++ "y"
  let _ = "z"
  println (twice s)
  println (show_int (ignore "w"))
  println (pick True "a")
  println (pick False "b")
