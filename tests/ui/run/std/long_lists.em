-- 末尾再帰で書いたリストの関数は、長いリストでも呼び出しの深さの上限に当たらない (docs/spec/stdlib.md)
count_down : Int -> List Int -> <Panic> List Int
count_down n acc = if n == 0 then acc else count_down (n - 1) (n :: acc)

main : Unit -> <IO, Panic> Unit
main () =
  let xs = count_down 100000 Nil
  let ys = List.map (fn x -> x + 1) xs
  println (show (List.length ys))
  println (show (List.fold (fn acc x -> acc + x) 0 (List.filter (fn x -> x % 2 == 0) ys)))
  println (show (List.length (List.append xs ys)))
