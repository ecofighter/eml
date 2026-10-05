-- An unused division by zero still stops the program: removing unused bindings keeps the ones that can fail.
main : Unit -> <IO> Unit
main () =
  let unused = 1 / 0
  println "unreachable"
