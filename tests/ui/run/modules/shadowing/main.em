-- A top-level definition hides an imported name and a Prelude name without a diagnostic; the qualified names still
-- reach the hidden definitions.
import Util (twice)

twice : Int -> Int
twice n = n + n + n

not : Bool -> Bool
not b = b

show_bool : Bool -> String
show_bool b = if b then "True" else "False"

main : Unit -> <IO> Unit
main () =
  println (show_int (twice 5))
  println (show_int (Util.twice 5))
  println (show_bool (not True))
  println (show_bool (Prelude.not True))
