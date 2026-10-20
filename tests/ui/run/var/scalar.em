-- `Unr` の変数: 数え上げ、`if` の枝での代入、範囲の中で呼ぶラムダからの読み出し
count_up : Int -> <Panic> Int
count_up n =
  var total = 0
  for 0 n (fn i -> total <- total + i)
  total

main : Unit -> <IO, Panic> Unit
main () =
  var x = 1
  x <- x + 1
  if x > 1 then x <- x * 10 else x <- 0
  println (show x)
  let ys = List.map (fn y -> y + x) [1, 2, 3]
  println (show ys)
  println (show (count_up 10))
