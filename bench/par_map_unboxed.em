-- `par_map.em`と組になる箱のない側。10万要素の`MutUnboxedArray Int`に、同じ軽い関数で
-- `Par.for_mut_unboxed`を使う。配列は走査ごとに`UnboxedArray.concat`を通して1回コピーされる
-- (docs/implementation/benchmarks.mdの「基準のプログラム」)。
import Std.MutUnboxedArray
import Std.Par
import Std.UnboxedArray

main : Unit -> <IO, Panic> Unit
main () =
  let xs = UnboxedArray.thaw (Par.tabulate_unboxed 100000 (fn i -> i))
  let ys = MutUnboxedArray.freeze (Par.for_mut_unboxed (fn _ x -> x + 1) xs)
  println (show (UnboxedArray.get ys 0 + UnboxedArray.get ys 99999 + UnboxedArray.length ys))
