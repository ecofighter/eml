-- 組の要素を入れ替える `List.map`。既知のラムダで特殊化した `map` は、リストの箱から組を開いて入れ替え、箱に戻す。
-- 葉の単位の覗き穴が `Float` の要素を箱のフィールドのまま渡し、手放す組の箱を reuse で使い回すので、確保と reuse の
-- 数は組が構造体になる前と同じである。数は `crates/eml_vm/tests/bench.rs` の `trade_offs` が固定する
-- (docs/spec/lir.md の「開く」)。
import Float
import List

main : Unit -> <IO, Panic> Unit
main () =
  let xs = List.map (fn n -> (n, Float.of_int n * 0.5)) [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
  let ys = List.map (fn (n, x) -> (x, n)) xs
  println (show (List.fold (fn acc (x, n) -> acc + x * Float.of_int n) 0.0 ys))
