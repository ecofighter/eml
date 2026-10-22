-- `Std.Float` の関数。期待値は Python 3.14 の同じ計算と同じである。変換と `to_fixed` の誤りは `Panic` なので、
-- `try_panic` が捕まえる。
import Float

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Float.of_int 3))
  println (show (Float.truncate (-2.7)))
  println (show (Float.floor (-2.5)))
  println (show (Float.ceil 2.1))
  println (show (Float.round (-0.5)))
  println (show (Float.sqrt 2.0))
  println (show (Float.abs (-0.0)))
  println (show (Float.sin 0.0))
  println (show (Float.cos 0.0))
  println (show (Float.exp 1.0))
  println (show (Float.log 1.0))
  println (show (Float.pow 2.0 10.0))
  println (show Float.pi)
  println (show (Float.is_nan Float.nan))
  println (show (Float.parse " 1.5 "))
  println (show (Float.parse "-inf"))
  println (show (Float.parse "1_0"))
  println (show (Float.parse "abc"))
  println (show (try_panic (fn () -> Float.truncate Float.nan)))
  println (show (try_panic (fn () -> Float.to_fixed 101 1.0)))
