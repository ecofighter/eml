-- Function values: calling function-typed parameters, passing top-level functions and builtins as values,
-- passing effects through a row variable, composition, and a polymorphic function used at an unboxed and a boxed type.
apply : (a -> <e> b) -> a -> <e> b
apply f x = f x

twice : (a -> a) -> a -> a
twice f x = f (f x)

inc : Int -> Int
inc n = n + 1

double : Int -> Int
double n = n * 2

id : a -> a
id x = x

main : Unit -> <IO> Unit
main () =
  println (show (apply inc 1))
  println (show (twice inc 5))
  apply println "through a row variable"
  println (show ((inc >> inc << inc) 0))
  println (show ((inc >> double) 1))
  println (show ((inc << double) 1))
  println (show (id 3))
  println (id "s")
