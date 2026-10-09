-- Polymorphic recursion: depth grows its type argument and runs on the uniform instance;
-- walk grows only `a`, so `b` is still specialized to Int.
depth : Int -> a -> Int
depth n x = if n == 0 then 0 else 1 + depth (n - 1) (x, x)

walk : Int -> a -> b -> b
walk n x y = if n == 0 then y else walk (n - 1) (x, x) y

main : Unit -> <IO> Unit
main () =
  println (show (depth 10 1))
  println (show (walk 10 "x" 42))
