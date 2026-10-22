-- 引き換えの形 (docs/implementation/benchmarks.md の「構造体の表現の引き換え」)。データに
-- 通した `List.map` は特殊化されず、一様な版を通って、データに通した関数の値を呼ぶ。関数の値は引数と結果が一様なので、
-- `map` は要素の組の箱をそのまま渡し、戻った箱をそのままリストに入れる。ラムダは箱から組を取り出して入れ替え、新しい
-- 箱に入れて返す (docs/spec/lir.md の「関数の ABI」)。数は `crates/eml_vm/tests/bench.rs` の `trade_offs` が固定する。
import Float
import List

data Box a = | Box a

hide : Int -> a -> <Panic> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x

main : Unit -> <IO, Panic> Unit
main () =
  let xs = List.map (fn n -> (n, Float.of_int n * 0.5)) [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
  let Box map = hide 0 List.map
  let Box swap = hide 0 (fn (n, x) -> (x, n))
  let ys = map swap xs
  println (show (List.fold (fn acc (x, n) -> acc + x * Float.of_int n) 0.0 ys))
