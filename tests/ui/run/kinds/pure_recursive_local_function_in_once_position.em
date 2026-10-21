-- 自分を呼ぶ局所関数も、row が純粋なら `once` の位置に渡せる。局所関数の型は読む式で広がる
-- (docs/spec/types.md の「値を読む式の矢印の線形性」)。
run_once : (once Bool -> Int) -> Int
run_once f = f False

main : Unit -> <IO> Unit
main () =
  let go b = if b then 1 else go True
  println (show (run_once go))
  println (show (go False))
