-- 型が `Array` の変数の添字は、変数を読んでから `Array.get` で読む
import Array

main : Unit -> <IO, Panic> Unit
main () =
  var best = Array.make 3 0
  println (show best[1])
  best <- Array.set best 1 5
  println (show best[1])
  println (show (Array.to_list best))
