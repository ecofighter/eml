-- ループの条件が `Int` の変数と配列の変数を `&&` で読む (`short_circuit_condition.em` と同じ形で、状態の1つが配列)
main : Unit -> <IO, Panic> Unit
main () =
  var i = 0
  let arr =
    var a = MutArray.make 10 0
    while (fn () -> i < 10 && a[i] == 0) (fn () ->
      a[i] := i * i
      i := i + 1)
    MutArray.freeze a
  println (show (Array.get arr 9))
  println (show i)
