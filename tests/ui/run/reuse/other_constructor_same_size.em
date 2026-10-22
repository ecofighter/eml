-- `List.map` が cons の箱を、同じ語の数の組の箱に使い回す。配置が違うので、ヘッダを書き直す。
import List

main : Unit -> <IO, Panic> Unit
main () =
  let pairs = List.map (fn x -> (x, x + 1)) [1, 2, 3]
  println "\{pairs}"
  let sums = List.map (fn (a, b) -> a + b) pairs
  println "\{sums}"
