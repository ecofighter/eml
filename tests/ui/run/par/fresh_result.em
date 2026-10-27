-- 右の子が返した値は、join で親の局所の物体に戻る (docs/spec/vm.md の「`par` の fork と join」)。右の子が作った
-- 木と、右の子が捕まえてそのまま返したリストを、親が作り直す。移し戻しを外すと、右の子の ID を退かせるときに
-- 検査モードが `TransferLeak` を返す。
import Std.Par

data Tree =
  | Leaf
  | Node Tree Int Tree

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

build : Int -> <Panic> Tree
build d = if d == 0 then Leaf else Node (build (d - 1)) d (build (d - 1))

incr : Tree -> <Panic> Tree
incr t = match t with
  | Leaf -> Leaf
  | Node l v r -> Node (incr l) (v + 1) (incr r)

total : Tree -> <Panic> Int
total t = match t with
  | Leaf -> 0
  | Node l v r -> total l + v + total r

upto : Int -> List Int -> <Panic> List Int
upto n acc = if n == 0 then acc else upto (n - 1) (n :: acc)

double : List Int -> <Panic> List Int
double xs = match xs with
  | [] -> []
  | x :: rest -> x * 2 :: double rest

main : Unit -> <IO, Panic> Unit
main () =
  let (n, t) = Par.par (fn () -> spin 40) (fn () -> build 4)
  println (show (n + total (incr t)))
  let xs = upto 5 []
  let (m, ys) = Par.par (fn () -> spin 40) (fn () -> xs)
  println (show (m, double ys))
