-- 自分を引数に受けて呼ぶ閉包で、本体が自分を4回呼ぶもの。融合の段は、書き換えで生まれた呼び出しを展開するたびに
-- 呼び出しが4倍に増えるが、関数ごとの大きさの予算 (`GROWTH_FACTOR`) で止まる。予算で残った呼び出しも、展開した
-- 所と同じ値を返す (docs/spec/core-ir.md の「融合が止まること」)
data Fix = | Fix (Fix -> Int -> Int)

main : Unit -> <IO> Unit
main () =
  let lam = fn self n -> match self with
    | Fix g -> if n <= 0 then 1 else g (Fix g) (n - 1) + g (Fix g) (n - 2) + g (Fix g) (n - 3) + g (Fix g) (n - 4)
  println (show (lam (Fix lam) 5))
  println (show (lam (Fix lam) 12))
