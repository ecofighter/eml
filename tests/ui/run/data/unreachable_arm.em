-- An arm after a catch-all is unreachable: a warning, and the program still runs.
data Color =
  | Red
  | Green

name : Color -> String
name c = match c with
  | _ -> "any"
  | Red -> "red"

main : Unit -> <IO> Unit
main () = println (name Green)
