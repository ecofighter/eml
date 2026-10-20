-- 最後の式の中でも、ラムダの中で `a` を取り出すので E3008
build : Unit -> <Panic> Array Int
build () =
  var a = MutArray.make 3 0
  a[0] <- 1
  (fn () -> MutArray.freeze a) ()

main : Unit -> <IO, Panic> Unit
main () = println (show (Array.length (build ())))
