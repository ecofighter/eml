-- 最後の式も変数の handler の中で動くので、`Unr` の変数は最後の式でも今の値を読む
pair : Int -> <Panic> (Array Int, Int)
pair n =
  var a = MutArray.make n 1
  var x = 0
  for 0 n (fn i ->
    a[i] := i * 2
    x := x + a[i])
  let peek () = x * 10
  (MutArray.freeze a, peek () + x)

main : Unit -> <IO, Panic> Unit
main () =
  var y = 1
  y := 5
  let (a, z) = pair 4
  println (show (Array.to_list a))
  println (show z)
  println (show (y + 1))
