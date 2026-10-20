-- 最後の式のタプルの両方の要素で `a` を取り出すので E3002
both : Unit -> <Panic> (Array Int, Array Int)
both () =
  var a = MutArray.make 3 0
  a[0] := 1
  (MutArray.freeze a, MutArray.freeze a)

main : Unit -> <IO, Panic> Unit
main () =
  let (x, y) = both ()
  println (show (Array.length x + Array.length y))
