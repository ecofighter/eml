-- 多相再帰の `twice_at` は型変数の位置が一様 (`_`) な instance で動き、受けた関数の値を一様な Repr で呼ぶ。
-- `main` のラムダは `Int`、`Float`、`String` を型どおりの Repr で受けて返すので、`apply` が実行時に引数と結果を
-- 変換する (docs/spec/lir.md の「`apply` の場所」)。
twice_at : Int -> (a -> <Panic> a) -> a -> <Panic> a
twice_at n f x =
  if n == 0 then f (f x)
  else
    match twice_at (n - 1) (fn p -> match p with | (y, u) -> (f y, u)) (x, ()) with
      | (y, _) -> y

main : Unit -> <IO, Panic> Unit
main () =
  println (show (twice_at 3 (fn n -> n * 2) 5))
  println (show (twice_at 2 (fn x -> x * 1.5) 2.0))
  println (twice_at 1 (fn s -> s ++ "!") "hi")
