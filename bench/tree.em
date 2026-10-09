-- 疑似乱数の `Int` を入れる二分探索木と、総称な畳み込み。
data Tree a =
  | Leaf
  | Node (Tree a) a (Tree a)

insert : Int -> Tree Int -> Tree Int
insert x t = match t with
  | Leaf -> Node Leaf x Leaf
  | Node l v r ->
    if x < v then Node (insert x l) v r
    else if x > v then Node l v (insert x r)
    else Node l v r

-- 線形合同法で次の種を作り、種の下位の桁を木に入れる
build : Int -> Int -> Tree Int -> Tree Int
build n seed t = if n == 0 then t else
  let next = (seed * 1103515245 + 12345) % 2147483648
  build (n - 1) next (insert (next % 1000000) t)

size : Tree a -> Int
size t = match t with
  | Leaf -> 0
  | Node l _ r -> size l + 1 + size r

fold : (b -> a -> b) -> b -> Tree a -> b
fold f acc t = match t with
  | Leaf -> acc
  | Node l v r -> fold f (f (fold f acc l) v) r

main : Unit -> <IO> Unit
main () =
  let t = build 15000 42 Leaf
  println (show_int (size t) ++ " " ++ show_int (fold (fn acc x -> acc + x) 0 t))
