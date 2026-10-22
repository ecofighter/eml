-- 最後の式の中で、取り出しのほかに `a[0]` で `a` を使うので E3008
import Array
import MutArray

build : Unit -> <Panic> (Int, Array Int)
build () =
  var a = MutArray.make 3 0
  a[0] <- 1
  (a[0], MutArray.freeze a)

main : Unit -> <IO, Panic> Unit
main () =
  let (n, b) = build ()
  println (show (n + Array.length b))
