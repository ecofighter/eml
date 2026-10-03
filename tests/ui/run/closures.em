-- Lambdas, closures that capture strings, a closure called twice, a last-argument lambda,
-- an unused polymorphic lambda, and deep recursion through closures.
each : Int -> (Int -> <e> Unit) -> <e> Unit
each n f =
  if n > 0 then
    f n
    each (n - 1) f

count_down : Int -> (Int -> Int) -> Int
count_down n k = if n == 0 then k 0 else count_down (n - 1) (fn m -> k (m + 1))

main : Unit -> <IO> Unit
main () =
  let suffix = "!"
  let shout = fn s -> s ++ suffix
  println (shout "hey")
  println (shout "you")
  each 3 fn n ->
    println ("n = " ++ show_int n)
  let unused = fn x -> x
  println (show_int (count_down 100000 (fn m -> m)))
