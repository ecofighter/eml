-- `orders.em` のラムダを各ループに書き込んだ手書きの版。自動の特殊化が目指す下限になる。
data Order = | Order { id : Int, qty : Int, price : Int }

make : Int -> <Panic> List Order
make n =
  let go i seed acc =
    if i == 0 then acc
    else
      let next = (seed * 1103515245 + 12345) % 2147483648
      go (i - 1) next (Order { id = i, qty = next % 10, price = next % 1000 } :: acc)
  go n 42 Nil

filter_big : List Order -> List Order
filter_big xs =
  let go acc ys =
    match ys with
      | Nil -> List.reverse acc
      | x :: rest -> if (x : Order).qty > 5 then go (x :: acc) rest else go acc rest
  go Nil xs

map_total : List Order -> <Panic> List Int
map_total xs =
  let go acc ys =
    match ys with
      | Nil -> List.reverse acc
      | Order { qty = q, price = p } :: rest -> go (q * p :: acc) rest
  go Nil xs

fold_sum : Int -> List Int -> <Panic> Int
fold_sum acc Nil = acc
fold_sum acc (x :: rest) = fold_sum (acc + x) rest

main : Unit -> <IO, Panic> Unit
main () =
  let orders = make 100000
  let big = filter_big orders
  let totals = map_total big
  let sum = fold_sum 0 totals
  println "\{sum}"
