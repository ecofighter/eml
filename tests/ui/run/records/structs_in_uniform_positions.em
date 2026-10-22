-- 構造体の値を一様な位置に置く。リストの要素の `(Int, Float)`、配列の要素の `Vec3`、`Option` の中の `Vec3` は、
-- どれも `con` と同じ形の箱に入り、取り出すと構造体に戻る
-- (docs/spec/lir.md の「構造体の形」)。
import Array
import Float
import List

data Vec3 =
  | Vec3 Float Float Float

sum_pairs : List (Int, Float) -> <Panic> Float
sum_pairs xs = match xs with
  | Nil -> 0.0
  | (n, x) :: rest -> Float.of_int n * x + sum_pairs rest

total : Vec3 -> <Panic> Float
total (Vec3 x y z) = x + y + z

find_big : List Vec3 -> <Panic> Option Vec3
find_big vs = match vs with
  | Nil -> None
  | v :: rest -> if total v > 5.0 then Some v else find_big rest

describe : Option Vec3 -> <IO, Panic> Unit
describe found = match found with
  | Some (Vec3 x y z) -> println (show x ++ " " ++ show y ++ " " ++ show z)
  | None -> println "none"

main : Unit -> <IO, Panic> Unit
main () =
  println (show (sum_pairs [(1, 0.5), (2, 1.25), (3, -2.0)]))
  let pairs = List.map (fn (n, x) -> (n * 10, x + 0.25)) [(1, 0.5), (2, 1.5)]
  println (show (sum_pairs pairs))
  let vs = Array.from_list [Vec3 1.0 2.0 3.0, Vec3 0.5 0.5 0.5]
  println (show (total vs[0] + total vs[1]))
  describe (find_big [Vec3 1.0 1.0 1.0, Vec3 2.0 2.5 3.0])
  describe (find_big [Vec3 0.0 0.0 0.0])
