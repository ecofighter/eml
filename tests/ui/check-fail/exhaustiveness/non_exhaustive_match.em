-- E4001: the `match` has no arm for `Blue`.
data Color = | Red | Green | Blue

name : Color -> String
name c = match c with
  | Red -> "red"
  | Green -> "green"

main : Unit -> <IO> Unit
main () = println (name Red)
