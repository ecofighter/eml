-- 10万要素の箱に入った`Array Int`に、軽い関数で`Par.map`を使う (98個の塊、97回のfork)。
-- この行は`par_map_unboxed.em`と組になる箱の側で、塊は要素ごとに参照を1つ取る`Array.concat`で結ぶ
-- (docs/implementation/benchmarks.mdの「基準のプログラム」)。
import Std.Array
import Std.Par

main : Unit -> <IO, Panic> Unit
main () =
  let xs = Par.tabulate 100000 (fn i -> i)
  let ys = Par.map (fn x -> x + 1) xs
  println (show (Array.get ys 0 + Array.get ys 99999 + Array.length ys))
