-- fixtures: only runnable through the test harness
-- fixture ids: 6..7
-- `MutUnboxedArray` の引数は C の関数が書く配列で、結果の組で返る。`pure` でも、結果の `Unit` を捨てるだけで呼び出しが
-- 消えると、書いた値が失われる。そのため `MutUnboxedArray` の引数を持つ呼び出しは消えない。

import MutUnboxedArray

extern "C" pure symbol "eml_fixture_fill" fill : MutUnboxedArray Float -> Int -> Float -> (Unit, MutUnboxedArray Float)

main : Unit -> <IO, Panic> Unit
main () =
  let m = MutUnboxedArray.make 4 0.0
  let ((), m) = fill m 4 1.5
  let (x, m) = MutUnboxedArray.read m 3
  let (y, m) = MutUnboxedArray.read m 1
  drop m
  println (show x)
  println (show y)
