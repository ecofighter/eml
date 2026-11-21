-- 集まりの関数は、粒度1024を下回る長さ、ちょうどの長さ、上回る長さで、また可変と箱なしの形でも、
-- 逐次の対応する関数と同じ配列を返す (docs/spec/stdlib.mdの「`Std.Par`」)。
import Std.Array
import Std.Float
import Std.List
import Std.MutArray
import Std.MutUnboxedArray
import Std.Par
import Std.UnboxedArray

same : Int -> <Panic> Bool
same n =
  let par = Par.tabulate n (fn i -> i * i)
  let seq = Array.tabulate n (fn i -> i * i)
  let equal i = i == n || (Array.get par i == Array.get seq i && equal (i + 1))
  Array.length par == n && equal 0

same_unboxed : Int -> <Panic> Bool
same_unboxed n =
  let par = Par.tabulate_unboxed n (fn i -> Float.of_int i * 0.5)
  let seq = UnboxedArray.tabulate n (fn i -> Float.of_int i * 0.5)
  let equal i = i == n || (UnboxedArray.get par i == UnboxedArray.get seq i && equal (i + 1))
  UnboxedArray.length par == n && equal 0

main : Unit -> <IO, Panic> Unit
main () =
  println (show (List.map same [0, 1, 1023, 1024, 1025, 2048, 2049, 3500]))
  println (show (List.map same_unboxed [0, 1, 1024, 1025, 3500]))
  let m = Par.for_mut (fn i x -> x + i) (MutArray.tabulate 2500 (fn i -> i))
  let (last, m) = MutArray.read m 2499
  println (show (last, Array.get (MutArray.freeze m) 1024))
  let u = Par.for_mut_unboxed (fn i x -> x + Float.of_int i) (MutUnboxedArray.tabulate 2500 (fn i -> 0.5))
  let (first, u) = MutUnboxedArray.read u 0
  println (show (first, UnboxedArray.get (MutUnboxedArray.freeze u) 2499))
  println (show (try_panic (fn () -> Array.length (Par.tabulate (0 - 1) (fn i -> i)))))
  println (show (Par.reduce (fn a b -> a + b) 42 (Array.from_list ([] : List Int))))
  println (show (try_panic (fn () -> UnboxedArray.length (Par.tabulate_unboxed (0 - 1) (fn i -> Float.of_int i)))))
  println (show (try_panic (fn () -> Array.length (Par.map (fn x -> if x == 3000 || x == 1100 then panic "at \{show x}" else x) (Par.tabulate 5000 (fn i -> i))))))
  let k = MutArray.freeze (Par.for_mut (fn i x -> x + i) (MutArray.tabulate 1500 (fn i -> 2 * i)))
  println (show (Array.get k 0, Array.get k 1023, Array.get k 1024, Array.get k 1499))
  let s = MutArray.freeze (Par.for_mut (fn i x -> x + i) (MutArray.tabulate 700 (fn i -> 1)))
  println (show (Array.get s 0, Array.get s 350, Array.get s 699))
