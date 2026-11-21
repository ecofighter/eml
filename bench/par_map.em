-- 10万要素の箱に入った`Array Int`に、軽い関数で`Par.map`を使う (98個の塊、97回のfork)。
-- この行は`par_map_unboxed.em`と組になる箱の側で、塊は`Array.concat`で結ぶ。要素が`Int`なので、
-- concatは参照を取らず、タグの検査だけをする
-- (docs/implementation/benchmarks.mdの「基準のプログラム」)。
import Std.Array
import Std.Par

main : Unit -> <IO, Panic> Unit
main () =
  let xs = Par.tabulate 100000 (fn i -> i)
  let ys = Par.map (fn x -> x + 1) xs
  println (show (Array.get ys 0 + Array.get ys 99999 + Array.length ys))
