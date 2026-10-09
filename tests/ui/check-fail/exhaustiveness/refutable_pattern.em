-- E4003: a `let` pattern and a lambda parameter that do not match `None`.
data Option a = | None | Some a

main : Unit -> <IO> Unit
main () =
  let Some n = Some 1
  let f = fn (Some m) -> m
  println (show (n + f (Some 2)))
