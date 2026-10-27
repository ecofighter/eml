-- 子はどの Repr の値も返せる。組は箱の値の組で作り、構造体の組は join の後で箱から出す
-- (docs/spec/core-ir.md の「`par` の呼び出し」)。
import Std.Par

data P = | P { name : String, age : Int } deriving (Show)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let (x, n) = Par.par (fn () -> 1.5 * 2.0) (fn () -> spin 40 + 2)
  println (show x)
  println (show n)
  let (c, s) = Par.par (fn () -> 'z') (fn () -> "par")
  println (show c)
  println s
  let (p, xs) = Par.par (fn () -> P { name = "a", age = spin 40 + 3 }) (fn () -> [1, 2, 3])
  println (show p)
  println (show xs)
  let (b, u) = Par.par (fn () -> spin 40 == 0) (fn () -> ())
  println (show (b, u))
