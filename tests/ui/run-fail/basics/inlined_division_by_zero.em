-- 小さな関数をインライン化しても、その中の割り算が起こす実行時エラーは、関数の中の位置を報告する
-- (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「インライン化」)。
div_by : Int -> Int -> Int
div_by d n = n / d + 1

main : Unit -> <IO> Unit
main () =
  let a = div_by 2 10
  let b = div_by 0 a
  println (show b)
