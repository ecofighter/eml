-- fixtures: only runnable through the test harness
-- fixture ids: 5..6
-- `UnboxedArray` の引数は、借りた配列の要素 0 へのポインタとして渡り、C の関数は読むだけである。

import UnboxedArray

extern "C" pure symbol "eml_fixture_dot" dot : UnboxedArray Float -> UnboxedArray Float -> Int -> Float

main : Unit -> <IO> Unit
main () =
  let a = UnboxedArray.from_list [1.0, 2.0, 3.0]
  let b = UnboxedArray.from_list [4.0, 5.0, 6.0]
  println (show (dot a b 3))
  println (show (dot a b 2))
