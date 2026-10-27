-- 完全な二分木の各節点に 1 を足す。上の深さ 4 までは `par` で左右に分け、その下は逐次に足す。左の子は木を
-- その場で作り直し、右の子は fork で共有になった木から新しく作る。join で戻った木を、親がもう一度その場で作り直す。
import Std.Par

data Tree =
  | Leaf
  | Node Tree Int Tree

build : Int -> Int -> <Panic> Tree
build d v = if d == 0 then Leaf else Node (build (d - 1) (v * 2)) v (build (d - 1) (v * 2 + 1))

incr : Tree -> <Panic> Tree
incr t = match t with
  | Leaf -> Leaf
  | Node l v r -> Node (incr l) (v + 1) (incr r)

pincr : Int -> Tree -> <Panic> Tree
pincr d t = match t with
  | Leaf -> Leaf
  | Node l v r ->
    if d == 0 then Node (incr l) (v + 1) (incr r)
    else
      let (l2, r2) = Par.par (fn () -> pincr (d - 1) l) (fn () -> pincr (d - 1) r)
      Node l2 (v + 1) r2

total : Tree -> <Panic> Int
total t = match t with
  | Leaf -> 0
  | Node l v r -> total l + v + total r

main : Unit -> <IO, Panic> Unit
main () =
  let t = incr (pincr 4 (build 12 1))
  println (show (total t))
