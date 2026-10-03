-- Function values: calling function-typed parameters, passing top-level functions and builtins as values,
-- passing effects through a row variable, and composition.
apply : (a -> <e> b) -> a -> <e> b
apply f x = f x

twice : (a -> a) -> a -> a
twice f x = f (f x)

inc : Int -> Int
inc n = n + 1

main : Unit -> <IO> Unit
main () =
  println (show_int (apply inc 1))
  println (show_int (twice inc 5))
  apply println "through a row variable"
  println (show_int ((inc >> inc << inc) 0))
