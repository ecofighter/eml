-- `Counter` は `Async.run` の外でも使える。`add` は足した後の値を返し、64 ビットで巻き戻る
-- (docs/spec/stdlib.md の「`Std.Atomic`」)。
import Atomic

main : Unit -> <IO> Unit
main () =
  let c = Atomic.new 9223372036854775806
  println (show (Atomic.add 1 c))
  println (show (Atomic.add 1 c))
  Atomic.store 0 c
  println (show (Atomic.add (-3) c))
  println (show (Atomic.load c))
