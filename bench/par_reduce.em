-- 10万個の添字について、`Float`の和を`Par.reduce_range`で求める (98個の塊、97回のfork)。
-- 配列は作らないので、この行は分割とコールバックの費用になる
-- (docs/implementation/benchmarks.mdの「基準のプログラム」)。
import Std.Float
import Std.Par

main : Unit -> <IO, Panic> Unit
main () =
  let sum = Par.reduce_range (fn a b -> a + b) 0.0 0 100000 (fn i -> 1.0 / Float.of_int (i + 1))
  println (Float.to_fixed 9 sum)
