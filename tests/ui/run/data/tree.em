-- A polymorphic binary tree with three fields per node. Insertion rebuilds the path, the in-order walk
-- visits every node, and the size works for any element type.
data Tree a =
  | Leaf
  | Node (Tree a) a (Tree a)

insert : Int -> Tree Int -> Tree Int
insert x t = match t with
  | Leaf -> Node Leaf x Leaf
  | Node l v r -> if x < v then Node (insert x l) v r else Node l v (insert x r)

walk : Tree Int -> String
walk t = match t with
  | Leaf -> ""
  | Node l v r -> walk l ++ show_int v ++ ";" ++ walk r

size : Tree a -> Int
size t = match t with
  | Leaf -> 0
  | Node l _ r -> size l + 1 + size r

main : Unit -> <IO> Unit
main () =
  let t = insert 5 (insert 2 (insert 8 (insert 1 (insert 9 Leaf))))
  println (walk t)
  println (show_int (size (Node Leaf "only" Leaf)))
