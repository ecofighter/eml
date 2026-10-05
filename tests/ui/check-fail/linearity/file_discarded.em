-- E3004: a file cannot be discarded with `_`.
main : Unit -> <IO> Unit
main () =
  let (_, text) = read_all (open "input.txt")
  println text
