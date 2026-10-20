-- 外側の `var x` を読む閉包を、内側の関数の自分の `var x` の範囲の中で呼ぶ。閉包は外側の `x` を読む
inner : (Unit -> <e> Int) -> <Panic | e> Int
inner read_outer =
  var x = 100
  x := x + 1
  read_outer () + x

main : Unit -> <IO, Panic> Unit
main () =
  var x = 1
  println (show (inner (fn () -> x)))
  x := 2
  println (show (inner (fn () -> x)))
