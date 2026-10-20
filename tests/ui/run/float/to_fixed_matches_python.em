-- `Float.to_fixed n x` は Python の `f"{x:.{n}f}"` と同じ文字列を返す。期待値は Python 3.14 で作った。
main : Unit -> <IO, Panic> Unit
main () =
  println (Float.to_fixed 0 2.5)
  println (Float.to_fixed 0 3.5)
  println (Float.to_fixed 2 1.005)
  println (Float.to_fixed 9 (1.0 / 3.0))
  println (Float.to_fixed 3 (-0.0))
  println (Float.to_fixed 2 Float.infinity)
  println (Float.to_fixed 2 Float.nan)
  println (Float.to_fixed 20 0.1)
