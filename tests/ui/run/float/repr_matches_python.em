-- `show` は Python の `repr` と同じ形で書く。期待値は Python 3.14 の `repr` で作った
-- (docs/spec/declarations.md の「Prelude のクラス」)。
import Float

main : Unit -> <IO, Panic> Unit
main () =
  println (show 0.1)
  println (show (1.0 / 3.0))
  println (show 1e16)
  println (show 1.5e16)
  println (show 1e15)
  println (show 123456.0)
  println (show 1e-5)
  println (show 0.0001)
  println (show (-0.0))
  println (show (1.0 / 0.0))
  println (show (-1.0 / 0.0))
  println (show (0.0 / 0.0))
  println (show 1e22)
  println (show 5e-324)
  println (show 1.7976931348623157e308)
  -- 2^-1017 と 2^89。2 の冪では往復できる区間が下側だけ狭く、正しく丸めた最短の桁では読み戻せない
  println (show 7.120236347223045e-307)
  println (show 6.189700196426902e26)
  println (show 2.5)
  println (show 3.5)
  println (show (Float.round 2.5))
  println (show (Float.round 3.5))
  println (show (Some (-1.5)))
  println (show [0.5, -0.5])
