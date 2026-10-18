-- 完全二分木の全節を `incr` で作り直すことを繰り返す。古い節は一意なので、作り直す節はその箱を使い回し、
-- 変わらない部分木のフィールドは書かない (docs/spec/lir.md の「reuse」)。
data Tree =
  | Leaf
  | Node Tree Int Tree

full : Int -> Int -> Tree
full d x = if d == 0 then Leaf else Node (full (d - 1) (2 * x)) x (full (d - 1) (2 * x + 1))

incr : Tree -> Tree
incr t = match t with
  | Leaf -> Leaf
  | Node l v r -> Node (incr l) (v + 1) (incr r)

times : Int -> Tree -> Tree
times n t = if n == 0 then t else times (n - 1) (incr t)

sum : Tree -> Int
sum t = match t with
  | Leaf -> 0
  | Node l v r -> sum l + v + sum r

main : Unit -> <IO> Unit
main () = println (show (sum (times 20 (full 13 1))))
