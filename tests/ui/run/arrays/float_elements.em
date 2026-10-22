-- `Float` の要素は箱に入った一様な値として配列に入る。
import Array
import Float

main : Unit -> <IO, Panic> Unit
main () =
  let a = Array.tabulate 4 (fn i -> Float.of_int i * 0.5)
  let sum i acc = if i == Array.length a then acc else sum (i + 1) (acc + Array.get a i)
  println (show (sum 0 0.0))
