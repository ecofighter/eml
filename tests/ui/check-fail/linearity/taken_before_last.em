-- 最後の式より前の文で `a` を取り出すので E3008
import Array
import MutArray

build : Unit -> <Panic> Array Int
build () =
  var a = MutArray.make 3 0
  let b = MutArray.freeze a
  b

main : Unit -> <IO, Panic> Unit
main () = println (show (Array.length (build ())))
