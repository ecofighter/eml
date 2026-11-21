-- `Array.concat`は要素の配列を順につなぐ。同じ配列が2度現れれば2度写し、空の配列は何も足さない。入力は使い続けられ、結果は新しい配列である (docs/spec/stdlib.mdの「`Std.Array`」)。
import Array

main : Unit -> <IO, Panic> Unit
main () =
  let a = Array.tabulate 3 (fn i -> "a\{show i}")
  let b = Array.from_list ([] : List String)
  let c = Array.tabulate 2 (fn i -> "c\{show i}")
  let chunks = Array.from_list [a, b, c, a]
  let all = Array.concat chunks
  println (show (Array.to_list all))
  println (show (Array.to_list (Array.set all 0 "x"), Array.to_list a, Array.length chunks))
  println (show (Array.length (Array.concat (Array.from_list ([] : List (Array Int))))))
  println (show (Array.to_list (Array.concat (Array.from_list [Array.tabulate 2 (fn i -> i), Array.make 1 7]))))
