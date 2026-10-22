-- 順に並べた集合 (docs/spec/stdlib.md の「`Std.Set`」)。`Std.Map` と同じ AVL 木で、節は値を持たない。左右の高さの差を
-- コンストラクタで表すので、算術の `Panic` がない。釣り合いを直す関数は、組み直す節を受け取って同じ関数の中で節を作る
-- (docs/spec/lir.md の「reuse」)
import List

pub data Set a =
  | Empty
  | LeftHigh (Set a) a (Set a)
  | Even (Set a) a (Set a)
  | RightHigh (Set a) a (Set a)

pub empty : Set a
empty = Empty

-- 左の部分木が高くなって、左が2高くなった節を回す。回した後の高さは、挿入の前と同じである
grow_left : Set a -> Set a
grow_left t =
  match t with
    | LeftHigh (LeftHigh a x b) y r -> Even a x (Even b y r)
    | LeftHigh (RightHigh a x (LeftHigh b z c)) y r -> Even (Even a x b) z (RightHigh c y r)
    | LeftHigh (RightHigh a x (RightHigh b z c)) y r -> Even (LeftHigh a x b) z (Even c y r)
    | LeftHigh (RightHigh a x (Even b z c)) y r -> Even (Even a x b) z (Even c y r)
    | _ -> t

grow_right : Set a -> Set a
grow_right t =
  match t with
    | RightHigh l y (RightHigh a x b) -> Even (Even l y a) x b
    | RightHigh l y (LeftHigh (RightHigh a z b) x c) -> Even (LeftHigh l y a) z (Even b x c)
    | RightHigh l y (LeftHigh (LeftHigh a z b) x c) -> Even (Even l y a) z (RightHigh b x c)
    | RightHigh l y (LeftHigh (Even a z b) x c) -> Even (Even l y a) z (Even b x c)
    | _ -> t

-- 結果と、部分木の高さが増えたか
ins : Ord a => a -> Set a -> (Set a, Bool)
ins x t =
  match t with
    | Empty -> (Even Empty x Empty, True)
    | LeftHigh l y r ->
      match compare x y with
        | LT ->
          let (l2, grew) = ins x l
          if grew then (grow_left (LeftHigh l2 y r), False) else (LeftHigh l2 y r, False)
        | GT ->
          let (r2, grew) = ins x r
          if grew then (Even l y r2, False) else (LeftHigh l y r2, False)
        | EQ -> (LeftHigh l y r, False)
    | Even l y r ->
      match compare x y with
        | LT ->
          let (l2, grew) = ins x l
          if grew then (LeftHigh l2 y r, True) else (Even l2 y r, False)
        | GT ->
          let (r2, grew) = ins x r
          if grew then (RightHigh l y r2, True) else (Even l y r2, False)
        | EQ -> (Even l y r, False)
    | RightHigh l y r ->
      match compare x y with
        | LT ->
          let (l2, grew) = ins x l
          if grew then (Even l2 y r, False) else (RightHigh l2 y r, False)
        | GT ->
          let (r2, grew) = ins x r
          if grew then (grow_right (RightHigh l y r2), False) else (RightHigh l y r2, False)
        | EQ -> (RightHigh l y r, False)

-- すでにある要素は、前からあるものを残す
pub insert : Ord a => a -> Set a -> Set a
insert x s =
  let (t, _) = ins x s
  t

-- 左の部分木が1低くなった節を直す。結果と、節の高さが減ったか
shrink_left : Set a -> (Set a, Bool)
shrink_left t =
  match t with
    | LeftHigh l x r -> (Even l x r, True)
    | Even l x r -> (RightHigh l x r, False)
    | RightHigh l x (Even a y b) -> (LeftHigh (RightHigh l x a) y b, False)
    | RightHigh l x (RightHigh a y b) -> (Even (Even l x a) y b, True)
    | RightHigh l x (LeftHigh (LeftHigh a z b) y c) -> (Even (Even l x a) z (RightHigh b y c), True)
    | RightHigh l x (LeftHigh (RightHigh a z b) y c) -> (Even (LeftHigh l x a) z (Even b y c), True)
    | RightHigh l x (LeftHigh (Even a z b) y c) -> (Even (Even l x a) z (Even b y c), True)
    | _ -> (t, False)

shrink_right : Set a -> (Set a, Bool)
shrink_right t =
  match t with
    | RightHigh l x r -> (Even l x r, True)
    | Even l x r -> (LeftHigh l x r, False)
    | LeftHigh (Even a y b) x r -> (RightHigh a y (LeftHigh b x r), False)
    | LeftHigh (LeftHigh a y b) x r -> (Even a y (Even b x r), True)
    | LeftHigh (RightHigh a y (LeftHigh b z c)) x r -> (Even (Even a y b) z (RightHigh c x r), True)
    | LeftHigh (RightHigh a y (RightHigh b z c)) x r -> (Even (LeftHigh a y b) z (Even c x r), True)
    | LeftHigh (RightHigh a y (Even b z c)) x r -> (Even (Even a y b) z (Even c x r), True)
    | _ -> (t, False)

-- いちばん小さい要素の節を外す。外した要素、残りの木、高さが減ったか。`d` は、届かない空の木の場合に返す値である
pop_min : Set a -> a -> (a, Set a, Bool)
pop_min t d =
  match t with
    | Empty -> (d, Empty, False)
    | LeftHigh l x r ->
      let (m, l2, shrunk) = pop_min l d
      if shrunk then
        let (t2, s) = shrink_left (LeftHigh l2 x r)
        (m, t2, s)
      else (m, LeftHigh l2 x r, False)
    | Even Empty x r -> (x, r, True)
    | Even l x r ->
      let (m, l2, shrunk) = pop_min l d
      if shrunk then
        let (t2, s) = shrink_left (Even l2 x r)
        (m, t2, s)
      else (m, Even l2 x r, False)
    | RightHigh Empty x r -> (x, r, True)
    | RightHigh l x r ->
      let (m, l2, shrunk) = pop_min l d
      if shrunk then
        let (t2, s) = shrink_left (RightHigh l2 x r)
        (m, t2, s)
      else (m, RightHigh l2 x r, False)

-- 結果と、部分木の高さが減ったか
del : Ord a => a -> Set a -> (Set a, Bool)
del x t =
  match t with
    | Empty -> (Empty, False)
    | LeftHigh l y r ->
      match compare x y with
        | LT ->
          let (l2, s) = del x l
          if s then shrink_left (LeftHigh l2 y r) else (LeftHigh l2 y r, False)
        | GT ->
          let (r2, s) = del x r
          if s then shrink_right (LeftHigh l y r2) else (LeftHigh l y r2, False)
        -- 左が高いので、左は空でない。右が空なら左の1節だけが残る
        | EQ ->
          match r with
            | Empty -> (l, True)
            | _ ->
              let (m, r2, s) = pop_min r y
              if s then shrink_right (LeftHigh l m r2) else (LeftHigh l m r2, False)
    | Even l y r ->
      match compare x y with
        | LT ->
          let (l2, s) = del x l
          if s then shrink_left (Even l2 y r) else (Even l2 y r, False)
        | GT ->
          let (r2, s) = del x r
          if s then shrink_right (Even l y r2) else (Even l y r2, False)
        | EQ ->
          match r with
            | Empty -> (l, True)
            | _ ->
              let (m, r2, s) = pop_min r y
              if s then shrink_right (Even l m r2) else (Even l m r2, False)
    | RightHigh l y r ->
      match compare x y with
        | LT ->
          let (l2, s) = del x l
          if s then shrink_left (RightHigh l2 y r) else (RightHigh l2 y r, False)
        | GT ->
          let (r2, s) = del x r
          if s then shrink_right (RightHigh l y r2) else (RightHigh l y r2, False)
        | EQ ->
          let (m, r2, s) = pop_min r y
          if s then shrink_right (RightHigh l m r2) else (RightHigh l m r2, False)

pub delete : Ord a => a -> Set a -> Set a
delete x s =
  let (t, _) = del x s
  t

pub member : Ord a => a -> Set a -> Bool
member x s =
  match s with
    | Empty -> False
    | LeftHigh l y r ->
      match compare x y with
        | LT -> member x l
        | GT -> member x r
        | EQ -> True
    | Even l y r ->
      match compare x y with
        | LT -> member x l
        | GT -> member x r
        | EQ -> True
    | RightHigh l y r ->
      match compare x y with
        | LT -> member x l
        | GT -> member x r
        | EQ -> True

-- 昇順にたたむ
pub fold : (b -> a -> <e> b) -> b -> Set a -> <e> b
fold f acc s =
  match s with
    | Empty -> acc
    | LeftHigh l x r -> fold f (f (fold f acc l) x) r
    | Even l x r -> fold f (f (fold f acc l) x) r
    | RightHigh l x r -> fold f (f (fold f acc l) x) r

pub size : Set a -> <Panic> Int
size s = fold (fn n _ -> n + 1) 0 s

-- 昇順の列
pub to_list : Set a -> List a
to_list s =
  let go t acc =
    match t with
      | Empty -> acc
      | LeftHigh l x r -> go l (x :: go r acc)
      | Even l x r -> go l (x :: go r acc)
      | RightHigh l x r -> go l (x :: go r acc)
  go s Nil

pub from_list : Ord a => List a -> Set a
from_list xs = List.fold (fn s x -> insert x s) empty xs

-- 木の高さを `1 <<< 高さ` で返す。印から高いほうをたどるので O(log n) で、シフトだけで数えるので `Panic` がない。
-- 集合の演算で、小さい側をたどるかを決めるのに使う
weight : Set a -> Int
weight t =
  match t with
    | Empty -> 1
    | LeftHigh l _ _ -> weight l <<< 1
    | Even l _ _ -> weight l <<< 1
    | RightHigh _ _ r -> weight r <<< 1

-- `x` と等しい要素を `x` で置き換えるか、入れる
replace : Ord a => a -> Set a -> Set a
replace x s =
  let (t, _) = ins x (delete x s)
  t

-- `x` と等しい要素
find : Ord a => a -> Set a -> Option a
find x s =
  match s with
    | Empty -> None
    | LeftHigh l y r -> find_at x l y r
    | Even l y r -> find_at x l y r
    | RightHigh l y r -> find_at x l y r

find_at : Ord a => a -> Set a -> a -> Set a -> Option a
find_at x l y r =
  match compare x y with
    | LT -> find x l
    | GT -> find x r
    | EQ -> Some y

-- 低いほうの木の要素を、高いほうの木に1つずつ入れる。等しい要素は `a` のものが残る
pub union : Ord a => Set a -> Set a -> Set a
union a b =
  if weight b <= weight a then fold (fn s x -> insert x s) a b
  else fold (fn s x -> replace x s) b a

-- 低いほうの木をたどり、もう一方にある要素を集める。等しい要素は `a` のものが残る
pub intersection : Ord a => Set a -> Set a -> Set a
intersection a b =
  if weight a <= weight b then fold (fn s x -> if member x b then insert x s else s) empty a
  else
    fold
      (fn s x ->
        match find x a with
          | Some y -> insert y s
          | None -> s)
      empty
      b

-- `a` から `b` の要素を除く。`b` が低ければ `b` の要素を1つずつ消し、そうでなければ `a` の要素のうち `b` にないものを集める
pub difference : Ord a => Set a -> Set a -> Set a
difference a b =
  if weight b <= weight a then fold (fn s x -> delete x s) a b
  else fold (fn s x -> if member x b then s else insert x s) empty a
