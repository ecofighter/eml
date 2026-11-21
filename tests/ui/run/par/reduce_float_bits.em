-- `Float`の和は結合的でないので、ビットは結ぶ順に依る。`Par.reduce_range`と`Par.reduce`は分割の順に結び、
-- その順は長さと粒度だけで決まるので、どのworkerの数でもどのスケジュールでも同じ桁が出る
-- (docs/spec/stdlib.mdの「`Std.Par`」)。
import Std.Array
import Std.Float
import Std.Par

term : Int -> <Panic> Float
term i = 1.0 / Float.of_int (i * i + 1)

add : Float -> Float -> <Panic> Float
add a b = a + b

sum_from : Int -> Int -> Float -> <Panic> Float
sum_from i n acc = if i == n then acc else sum_from (i + 1) n (acc + term i)

main : Unit -> <IO, Panic> Unit
main () =
  let n = 5000
  let by_range = Par.reduce_range add 0.0 0 n term
  let by_array = Par.reduce add 0.0 (Par.tabulate n term)
  println (show by_range)
  println (show (by_range == by_array))
  println (show (Par.reduce_range_grain 7 add 0.0 0 1000 term))
  println (show (sum_from 0 n 0.0))
