-- 共有された配列の `thaw` は写すので、その後の `fill` で元の配列は変わらない。一意な配列の `thaw` は写さず、`fill` は
-- その場で書く。
import MutUnboxedArray
import UnboxedArray

main : Unit -> <IO, Panic> Unit
main () =
  let a = UnboxedArray.set (UnboxedArray.make 3 0.0) 1 5.0
  let m = MutUnboxedArray.fill (UnboxedArray.thaw a) 7.0
  let (x, m) = MutUnboxedArray.read m 0
  let filled = MutUnboxedArray.freeze m
  println (show (x, UnboxedArray.to_list filled, UnboxedArray.to_list a))
  let unique = MutUnboxedArray.fill (UnboxedArray.thaw (UnboxedArray.make 2 1)) 3
  let (y, unique) = MutUnboxedArray.read unique 1
  println (show (y, UnboxedArray.to_list (MutUnboxedArray.freeze unique)))
