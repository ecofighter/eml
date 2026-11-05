-- クラスの制約で要素の型を抽象した関数を、`Float` と `Int` の両方の配列に使う。
import Float
import MutUnboxedArray
import UnboxedArray

sum_by : UnboxedArray.Unboxed a => (a -> Float) -> UnboxedArray a -> <Panic> Float
sum_by f a =
  let go i acc =
    if i == UnboxedArray.length a then acc else go (i + 1) (acc + f (UnboxedArray.get a i))
  go 0 0.0

-- `read` と `write` でその場で並びを逆にする。部分適用した `read` は組を作る包む関数になる
reverse : MutUnboxedArray.MutUnboxed a => MutUnboxedArray a -> <Panic> MutUnboxedArray a
reverse m =
  let swap i j m =
    if i >= j then m
    else
      let (x, m) = MutUnboxedArray.read m i
      let read_m = MutUnboxedArray.read m
      let (y, m) = read_m j
      swap (i + 1) (j - 1) (MutUnboxedArray.write (MutUnboxedArray.write m i y) j x)
  let (n, m) = MutUnboxedArray.length m
  swap 0 (n - 1) m

main : Unit -> <IO, Panic> Unit
main () =
  let fs = UnboxedArray.from_list [0.5, 1.5, 2.0]
  let is = UnboxedArray.from_list [1, 2, 3, 4]
  println (show (sum_by (fn x -> x) fs, sum_by Float.of_int is))
  let rf = MutUnboxedArray.freeze (reverse (UnboxedArray.thaw fs))
  let ri = MutUnboxedArray.freeze (reverse (UnboxedArray.thaw is))
  println (show (UnboxedArray.to_list rf, UnboxedArray.to_list ri))
  println (show (UnboxedArray.to_list fs, UnboxedArray.to_list is))
