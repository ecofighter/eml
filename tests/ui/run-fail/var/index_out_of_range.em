-- 配列の変数の範囲外の添字を誰も捕まえないと、実行時エラーになり、添字を書いた所の位置を出す
main : Unit -> <IO, Panic> Unit
main () =
  var a = MutArray.make 3 0
  a[0] <- 1
  println (show a[5])
  drop a
