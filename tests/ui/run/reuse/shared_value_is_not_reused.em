-- 作り直す値に別の参照が残っていると、token は空になり、`con` は新しい箱を確保する。古い木と古いレコードは
-- 変わらずに残る。
import List

data Tree =
  | Leaf
  | Node Tree Int Tree

data Stats = | Stats { count : Int, total : Int } deriving (Show)

insert : Int -> Tree -> Tree
insert x t =
  match t with
    | Leaf -> Node Leaf x Leaf
    | Node l v r ->
      if x < v then Node (insert x l) v r
      else if x > v then Node l v (insert x r)
      else t

to_list : Tree -> List Int
to_list t =
  match t with
    | Leaf -> []
    | Node l v r -> List.append (to_list l) (v :: to_list r)

build : List Int -> Tree -> Tree
build xs t =
  match xs with
    | [] -> t
    | x :: rest -> build rest (insert x t)

bump : Int -> Stats -> <Panic> Stats
bump n st = if n == 0 then st else bump (n - 1) { st | count = st.count + 1, total = st.total + n }

main : Unit -> <IO, Panic> Unit
main () =
  let old = build [5, 8, 2] Leaf
  let new = build [9, 1, 6] old
  println "old \{to_list old}"
  println "new \{to_list new}"
  let first = bump 3 (Stats { count = 0, total = 0 })
  let second = bump 2 first
  println "first \{first}"
  println "second \{second}"
