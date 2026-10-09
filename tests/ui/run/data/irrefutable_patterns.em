-- Single-constructor patterns in a `let`, a lambda parameter and an equation parameter.
data Box a =
  | Box a

unbox : Box String -> String
unbox (Box s) = s

main : Unit -> <IO> Unit
main () =
  let Box n = Box 41
  println (show (n + 1))
  let bang = fn (Box s) -> s ++ "!"
  println (bang (Box "lambda"))
  println (unbox (Box "equation"))
