-- `let` の局所変数には代入できない。E1053
main : Unit -> <IO> Unit
main () =
  let n = 1
  n <- 2
  println "\{n}"
