-- `par` は値としても使える。値の `par` は `$externN` の包みで、その本体も `par` の命令である
-- (docs/spec/core-ir.md の「`par` の呼び出し」)。
import Std.Par

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

apply2 : ((Unit -> <e> Int) -> (Unit -> <e> Int) -> <e> (Int, Int)) -> (Unit -> <e> Int) -> <e> (Int, Int)
apply2 p f = p f f

main : Unit -> <IO, Panic> Unit
main () =
  let p = Par.par
  let (a, b) = p (fn () -> spin 40 + 1) (fn () -> 2)
  println (show (a + b))
  let (c, d) = apply2 Par.par (fn () -> spin 40 + 3)
  println (show (c + d))
  let q = Par.par (fn () -> if spin 40 == 0 then 0.25 else 0.5)
  println (show (q (fn () -> 7)))
