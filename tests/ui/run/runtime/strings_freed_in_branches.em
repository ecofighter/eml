-- Strings chosen by `if`, built by a function, used twice, passed to an unused parameter, and discarded
-- must all be freed.
twice : String -> String
twice s = s ++ s

greet : String -> String
greet name = "Hello, " ++ name ++ "!"

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
  println (show (ignore "w"))
  println (pick True "a")
  println (pick False "b")
  let name = "eml"
  println (greet name)
  println name
