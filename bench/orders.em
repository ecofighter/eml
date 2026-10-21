-- 注文のレコードを、ラムダを渡した `List.filter` / `List.map` / `List.fold` で集計する。
-- 既知の閉包への特殊化が、`orders_by_hand.em` に近づける対象である。
data Order = | Order { id : Int, qty : Int, price : Int }

make : Int -> <Panic> List Order
make n =
  let go i seed acc =
    if i == 0 then acc
    else
      let next = (seed * 1103515245 + 12345) % 2147483648
      go (i - 1) next (Order { id = i, qty = next % 10, price = next % 1000 } :: acc)
  go n 42 Nil

main : Unit -> <IO, Panic> Unit
main () =
  let orders = make 100000
  let big = List.filter (fn o -> o.qty > 5) orders
  let totals = List.map (fn o -> o.qty * o.price) big
  let sum = List.fold (fn a b -> a + b) 0 totals
  println "\{sum}"
