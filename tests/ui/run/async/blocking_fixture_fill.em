-- fixtures: only runnable through the test harness
-- fixture ids: 1010..1011
-- `MutUnboxedArray` の引数を持つ `blocking` の呼び出しは、プールのスレッドが配列に書き、結果の組で配列を返す。
-- 書いた値はタスクが読み直せる。

import Async (Async)
import MutUnboxedArray

extern "C" blocking symbol "eml_fixture_fill" fill : MutUnboxedArray Float -> Int -> Float -> <IO, Async> (Unit, MutUnboxedArray Float)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    let m = MutUnboxedArray.make 4 0.0
    let ((), m) = fill m 4 1.5
    let (x, m) = MutUnboxedArray.read m 3
    let (y, m) = MutUnboxedArray.read m 1
    drop m
    x + y)
  let _ = spin 40
  println (show (Async.await t))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
