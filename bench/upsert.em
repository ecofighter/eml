-- `logstat` の AA 木の `upsert key increment` で文字列のキーを数え、件数の降順に `sort_by` する。
-- 高階の `f` と比較関数を、既知クロージャの特殊化が呼び出し先に埋め込む対象である。

-- 釣り合いを取った二分探索木 (AA 木)。`Node` の最初の欄は節の高さの段である。パスに ID を含むログでは、キーが
-- 昇順に増えるので、釣り合いを取らないと挿入が全体で2乗の時間になる
import Std.List

data Tree k v =
  | Leaf
  | Node Int (Tree k v) k v (Tree k v)

-- 左の子が同じ段なら、右に回す
skew : Tree k v -> Tree k v
skew tree =
  match tree with
    | Node level (Node left_level a lk lv b) k v right ->
      if left_level == level then Node level a lk lv (Node level b k v right) else tree
    | _ -> tree

-- 右の孫が同じ段なら、右の子を1段上げる
split : Tree k v -> <Panic> Tree k v
split tree =
  match tree with
    | Node level a k v (Node right_level b rk rv (Node grand_level c gk gv d)) ->
      if grand_level == level then
        Node (right_level + 1) (Node level a k v b) rk rv (Node grand_level c gk gv d)
      else tree
    | _ -> tree

-- `k` の値を `f` で更新する。なければ `f None` を入れる
upsert : Ord k => k -> (Option v -> <Panic> v) -> Tree k v -> <Panic> Tree k v
upsert key f tree =
  match tree with
    | Leaf -> Node 1 Leaf key (f None) Leaf
    | Node level left k v right ->
      match compare key k with
        | LT -> split (skew (Node level (upsert key f left) k v right))
        | GT -> split (skew (Node level left k v (upsert key f right)))
        | EQ -> Node level left k (f (Some v)) right

-- キーの昇順の組の列
entries : Tree k v -> List (k, v)
entries tree =
  let go t acc =
    match t with
      | Leaf -> acc
      | Node _ left k v right -> go left ((k, v) :: go right acc)
  go tree Nil

-- 安定なマージソート。`before a b` は `a` を `b` より前に置くときに `True` である
sort_by : (a -> a -> Bool) -> List a -> <Panic> List a
sort_by before xs =
  let merge acc left right =
    match (left, right) with
      | (Nil, _) -> List.append (List.reverse acc) right
      | (_, Nil) -> List.append (List.reverse acc) left
      | (l :: ls, r :: rs) ->
        if before r l then merge (r :: acc) left rs else merge (l :: acc) ls right
  let n = List.length xs
  if n <= 1 then xs
  else
    let (left, right) = split_at (n / 2) xs
    merge Nil (sort_by before left) (sort_by before right)

split_at : Int -> List a -> <Panic> (List a, List a)
split_at n xs =
  let go i acc rest =
    match rest with
      | x :: more -> if i > 0 then go (i - 1) (x :: acc) more else (List.reverse acc, rest)
      | Nil -> (List.reverse acc, Nil)
  go n Nil xs

take : Int -> List a -> <Panic> List a
take n xs = (split_at n xs).0

-- 件数を1増やす (なければ1にする)
increment : Option Int -> <Panic> Int
increment old =
  match old with
    | None -> 1
    | Some n -> n + 1

-- 疑似乱数から作った20000個のキー (種類は最大2000個) を数える
count_keys : Int -> Int -> Tree String Int -> <Panic> Tree String Int
count_keys n seed tree =
  if n == 0 then tree
  else
    let next = (seed * 1103515245 + 12345) % 2147483648
    let key = "k" ++ show ((next / 65536) % 2000)
    count_keys (n - 1) next (upsert key increment tree)

main : Unit -> <IO, Panic> Unit
main () =
  let tree = count_keys 20000 42 Leaf
  let by_count = sort_by (fn (_, a) (_, b) -> a > b) (entries tree)
  println "\{List.length by_count} \{match by_count with | (k, v) :: _ -> "\{k} \{v}" | Nil -> ""}"
