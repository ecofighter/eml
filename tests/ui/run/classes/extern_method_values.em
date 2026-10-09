-- Prelude methods bound to externs, used as values and partially applied. Each such reference is
-- wrapped in a function that calls the extern.
apply : (a -> <e> b) -> a -> <e> b
apply f x = f x

shown : Show a => a -> String
shown x = apply show x

main : Unit -> <IO> Unit
main () =
  let first = compare 1
  println (shown 1 ++ " " ++ shown "a" ++ " " ++ shown True)
  println (show (first 2) ++ " " ++ show (apply ((==) "x") "x"))
