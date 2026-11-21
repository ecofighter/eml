-- `UnboxedArray.concat`は`Float`と`Int`の要素の配列を順につなぐ。入力は使い続けられる (docs/spec/stdlib.mdの「`Std.UnboxedArray`」)。
import Array
import Float
import UnboxedArray

main : Unit -> <IO, Panic> Unit
main () =
  let u = UnboxedArray.tabulate 3 (fn i -> 1.5 * Float.of_int i)
  let v = UnboxedArray.from_list ([] : List Float)
  let w = UnboxedArray.tabulate 2 (fn i -> 0.0 - Float.of_int i)
  let all = UnboxedArray.concat (Array.from_list [u, v, w, u])
  println (show (UnboxedArray.to_list all))
  println (show (UnboxedArray.to_list (UnboxedArray.set all 0 9.0), UnboxedArray.to_list u))
  let ints = UnboxedArray.concat (Array.from_list [UnboxedArray.tabulate 2 (fn i -> i), UnboxedArray.make 1 7])
  println (show (UnboxedArray.to_list ints))
  println (show (UnboxedArray.length (UnboxedArray.concat (Array.from_list ([] : List (UnboxedArray Int))))))
