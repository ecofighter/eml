-- `let ... in` is a one-line form of a block `let`.
main : Unit -> <IO> Unit
main () =
  let n = 20
  println (show_int (let x = n + 1 in x * 2))
