-- `Array` とリストとの変換は、空の配列でも `Panic` を起こさない。`Float` の要素は生のビットのまま往復するので、
-- `-0.0` と無限大も変わらない。
import Array
import Float
import MutUnboxedArray
import UnboxedArray

no_floats : List Float
no_floats = []

empty_ints : Unit -> MutUnboxedArray Int
empty_ints () = MutUnboxedArray.empty ()

main : Unit -> <IO, Panic> Unit
main () =
  let u = UnboxedArray.of_array (Array.from_list [1.0, 2.0, 3.0])
  println (show (UnboxedArray.length u, Array.to_list (UnboxedArray.to_array u)))
  let empty = UnboxedArray.of_array (Array.from_list no_floats)
  println (show (UnboxedArray.length empty, Array.to_list (UnboxedArray.to_array empty)))
  println (show (UnboxedArray.to_list (UnboxedArray.from_list [3, 1, 2])))
  println (show (UnboxedArray.to_list (UnboxedArray.from_list no_floats)))
  let e = empty_ints ()
  let (n, e) = MutUnboxedArray.length e
  println (show (n, UnboxedArray.to_list (MutUnboxedArray.freeze e)))
  let special = UnboxedArray.from_list [-0.0, Float.infinity, -Float.infinity]
  let back = UnboxedArray.to_list (UnboxedArray.of_array (UnboxedArray.to_array special))
  println (show back)
  match back with
    | z :: _ -> println (show (1.0 / z))
    | Nil -> println "empty"
