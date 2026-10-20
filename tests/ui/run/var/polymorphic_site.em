-- 総称的な関数の1つの `var` の地点を `Int` と `String` で使う。操作の表は地点ごとに1つなので、instance ごとに
-- box の挿入が変換する
remember : a -> a -> <Panic> a
remember x y =
  var last = x
  for 0 3 (fn _ -> last := y)
  last

main : Unit -> <IO, Panic> Unit
main () =
  println (show (remember 1 2))
  println (remember "a" "b")
