-- E3004: a file cannot be discarded with `_`.
main : Unit -> <IO> Unit
main () =
  let (text, _) = Fs.read_all (Fs.open "input.txt")
  println text
