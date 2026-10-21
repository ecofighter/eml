-- 呼び出しの規約の引き換えの形 (docs/implementation/benchmarks.md の「呼び出しの規約の引き換え」)。データに通した
-- `List.map` は特殊化されず、一様な版を通って、データに通した関数の値を呼ぶ。関数の値は引数と結果が一様なので、`map`
-- はリストの要素の箱をそのまま渡す。`keep` は箱から `Float` を取り出して比べ、結果を箱に入れて返す
-- (docs/spec/lir.md の「関数の ABI」)。要素ごとの箱の数は `crates/eml_vm/tests/bench.rs` の `trade_offs` が固定する。
data Box a = | Box a

hide : Int -> a -> <Panic> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x

main : Unit -> <IO, Panic> Unit
main () =
  let xs = [0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5, 8.5, 9.5]
  let Box map = hide 0 List.map
  let Box keep = hide 0 (fn x -> if x > 1.0 then x else 0.0)
  let ys = map keep xs
  println (show (List.fold (fn a b -> a + b) 0.0 ys))
