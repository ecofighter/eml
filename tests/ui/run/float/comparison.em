-- 比較の演算子は IEEE 754 のとおりで、NaN が絡むと `!=` のほかは偽になる。`compare` は全順序 (`f64::total_cmp`)
-- で、`-0.0` は `0.0` より小さい。`0.0 / 0.0` の NaN は、どのプラットフォームでも正の quiet NaN にそろうので、どの値
-- より大きい。導出した `Ord` の `<` は `compare` を通るので、複合の値の
-- 大小は全順序に従う。`==` はフィールドごとの `==` なので IEEE のままである。
main : Unit -> <IO, Panic> Unit
main () =
  let nan = 0.0 / 0.0
  println (show (nan == nan))
  println (show (nan != nan))
  println (show (nan < 1.0))
  println (show (nan > 1.0))
  println (show (nan <= nan))
  println (show (0.0 == -0.0))
  println (show (-0.0 < 0.0))
  println (show (1.0 < 2.0 && 2.0 <= 2.0 && 3.0 >= 2.0))
  println (show (compare (-0.0) 0.0))
  println (show (compare 1.0 nan))
  println (show (compare nan nan))
  println (show (compare (1.0 / 0.0) nan))
  println (show (compare 2.0 1.0))
  println (show (Some (-0.0) < Some 0.0))
  println (show ((0.0, 1) < (-0.0, 2)))
  println (show (Some nan == Some nan))
