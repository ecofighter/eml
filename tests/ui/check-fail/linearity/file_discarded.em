-- E3004: a file cannot be discarded with `_`.
main : Unit -> <IO> Unit
main () =
  let (_, text) = Fs.read_all (Fs.open "input.txt")
  println text
