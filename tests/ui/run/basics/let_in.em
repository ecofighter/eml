-- `let ... in` is a one-line form of a block `let`.
main : Unit -> <IO, Panic> Unit
main () =
  let n = 20
  println (show (let x = n + 1 in x * 2))
