-- 配列の変数を3本使い、最後の式でそれぞれを `freeze` する
build : Int -> <Panic> (Array Int, Array Int, Array Int)
build n =
  var a = MutArray.make n 0
  var b = MutArray.make n 0
  var c = MutArray.make n 0
  for 0 n (fn i ->
    a[i] <- i
    b[i] <- i * i)
  for 0 n (fn i -> c[i] <- a[i] + b[i])
  (MutArray.freeze a, MutArray.freeze b, MutArray.freeze c)

main : Unit -> <IO, Panic> Unit
main () =
  let (a, b, c) = build 5
  println (show (Array.to_list a))
  println (show (Array.to_list b))
  println (show (Array.to_list c))
