-- 関数を要素に持つ組を2回使い、組の要素を `once` の位置に渡せる。組の要素も、読む式で1段だけ広がる
-- (docs/spec/types.md の「値を読む式の矢印の線形性」)。
run_p : (once Int -> Int, Int) -> Int
run_p p =
  match p with
    | (f, n) -> f n

main : Unit -> <IO> Unit
main () =
  let g = fn x -> x
  let p = (g, 3)
  println (show (run_p p))
  println (show (run_p p))
