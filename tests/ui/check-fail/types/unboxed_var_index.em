-- 添字への書き込みは変数の型を `MutArray` と合わせるので、`UnboxedArray` の変数では E2001
import UnboxedArray

main : Unit -> <IO, Panic> Unit
main () =
  var u = UnboxedArray.make 3 1.0
  u[0] <- 2.0
  println (show (UnboxedArray.length u))
