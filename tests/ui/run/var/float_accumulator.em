-- `Float` の変数を、もう1本の変数と一緒に `for` のループで書き換える
main : Unit -> <IO, Panic> Unit
main () =
  var sum = 0.0
  var count = 0
  for 0 10 (fn i ->
    sum <- sum + Float.of_int i * 0.5
    count <- count + 1)
  println (show sum)
  println (show count)
