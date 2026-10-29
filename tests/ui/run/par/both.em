-- `both` は左、右の順に呼ぶ逐次の関数で、どの row の計算も受け付ける (docs/spec/stdlib.md の「`Std.Par`」)。子は `IO`
-- を起こせ、左の子の中断は右の子を走らせずに抜ける。`par` の子の中でも使える。
import Std.Par
import Fail (Fail(..))

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

noisy : String -> Int -> <IO> Int
noisy s n =
  println s
  n

main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.both (fn () -> noisy "left" 1) (fn () -> noisy "right" 2)
  println (show (a + b))
  let r = Fail.to_result (fn () -> Par.both (fn () -> (Fail.fail "stop" : Int)) (fn () -> noisy "never" 3))
  println (show r)
  let (c, (d, e)) = Par.par (fn () -> spin 40 + 1) (fn () -> Par.both (fn () -> 2) (fn () -> 3))
  println (show (c, d, e))
