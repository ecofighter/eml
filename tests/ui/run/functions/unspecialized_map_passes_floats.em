-- 型から決める呼び出しの規約の引き換えの形 (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の
-- 「引き換えにすること」)。データに通した `List.map` は特殊化されず、データに通した関数の値を型どおりの `Float` で
-- 呼ぶ。`keep` が受けた `Float` をそのまま返す要素でも、`map` はリストの要素の箱から取り出して渡し、戻った値を新しい
-- 箱に入れる。要素ごとの箱の数は `crates/eml_vm/tests/bench.rs` の `trade_offs` が固定する。
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
