-- `Int` の要素は 64 ビットの語をそのまま持つので、最大と最小の値も `set`、`thaw`、`write`、`read`、`freeze` の
-- 往復で変わらない。
import MutUnboxedArray
import UnboxedArray

main : Unit -> <IO, Panic> Unit
main () =
  let lo = -9223372036854775807 - 1
  let a = UnboxedArray.from_list [9223372036854775807, lo, -1]
  println (show (UnboxedArray.to_list a))
  let b = UnboxedArray.set a 2 lo
  println (show (UnboxedArray.to_list b, UnboxedArray.get a 2))
  let m = UnboxedArray.thaw b
  let m = MutUnboxedArray.write m 0 0
  let (x, m) = MutUnboxedArray.read m 1
  let c = MutUnboxedArray.freeze m
  println (show (x, UnboxedArray.to_list c, UnboxedArray.to_list b))
