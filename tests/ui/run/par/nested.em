-- 木の和を、深さ 4 まで `par` で分けて求める。右の子の中の `par` の左の子の中断は、その右の子の中の `to_result` が
-- 捕まえる。join は内側から順に終わり、終わった子の worker ID は次の fork で使い回す
-- (docs/spec/object-model.md の「論理的な worker ID」)。
import Std.Par
import Fail (Fail(..))

data Tree =
  | Leaf
  | Node Tree Int Tree

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

build : Int -> Int -> <Panic> Tree
build d v = if d == 0 then Leaf else Node (build (d - 1) (v * 2)) v (build (d - 1) (v * 2 + 1))

total : Tree -> <Panic> Int
total t = match t with
  | Leaf -> 0
  | Node l v r -> total l + v + total r

psum : Int -> Tree -> <Panic> Int
psum d t = match t with
  | Leaf -> 0
  | Node l v r ->
    if d == 0 then total t
    else
      let (a, b) = Par.par (fn () -> spin 20 + psum (d - 1) l) (fn () -> psum (d - 1) r)
      a + v + b

main : Unit -> <IO, Panic> Unit
main () =
  let t = build 6 1
  println (show (psum 4 t))
  println (show (total t))
  let (x, r) =
    Par.par (fn () -> spin 40 + 1) (fn () ->
      Fail.to_result (fn () -> Par.par (fn () -> spin 40 + Fail.fail "inner") (fn () -> 2)))
  println (show (x, r))
