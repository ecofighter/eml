-- 引き換えの形 (docs/implementation/benchmarks.md の「S14e-2」)。データに
-- 通した `List.map` は特殊化されず、データに通した関数の値を型どおりに、組を構造体の葉の並びで呼ぶ。`map` は要素の
-- 箱から組を取り出して渡し、戻った組を新しい箱に入れる。数は `crates/eml_vm/tests/bench.rs` の `trade_offs` が
-- 固定する。
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
