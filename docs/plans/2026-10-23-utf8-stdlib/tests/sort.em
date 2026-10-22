-- 安定な並べ替え (docs/spec/stdlib.md の「`List`」)。等しい要素は元の順に残る
import List

main : Unit -> <IO, Panic> Unit
main () =
  println (show (List.sort [5, 3, 9, 1, 3, 0, 2]))
  println (show (List.sort (Nil : List Int), List.sort [7]))
  let pairs = [("a", 1), ("b", 2), ("c", 1), ("d", 2), ("e", 3)]
  println (show (List.sort_by (fn (_, x) (_, y) -> compare y x) pairs))
  println (show (List.sort_by (fn (_, x) (_, y) -> compare x y) pairs))
  println (show (List.sort ["pear", "apple", "fig"]))
