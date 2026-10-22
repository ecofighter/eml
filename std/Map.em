-- キーの順に並べた写像 (docs/spec/stdlib.md の「`Std.Map`」)。AVL 木で、左右の高さの差をコンストラクタで表す
-- (`LeftHigh` は左が1高く、`Even` は同じで、`RightHigh` は右が1高い)。高さを数で持たないので、算術の `Panic` がない。
-- 釣り合いを直す関数は、組み直す節を受け取って同じ関数の中で節を作る (docs/spec/lir.md の「reuse」)
import List

pub data Map k v =
  | Empty
  | LeftHigh (Map k v) k v (Map k v)
  | Even (Map k v) k v (Map k v)
  | RightHigh (Map k v) k v (Map k v)

pub empty : Map k v
empty = Empty

-- 左の部分木が高くなって、左が2高くなった節を回す。回した後の高さは、挿入の前と同じである
grow_left : Map k v -> Map k v
grow_left t =
  match t with
    | LeftHigh (LeftHigh a ak av b) k v r -> Even a ak av (Even b k v r)
    | LeftHigh (RightHigh a ak av (LeftHigh b bk bv c)) k v r -> Even (Even a ak av b) bk bv (RightHigh c k v r)
    | LeftHigh (RightHigh a ak av (RightHigh b bk bv c)) k v r -> Even (LeftHigh a ak av b) bk bv (Even c k v r)
    | LeftHigh (RightHigh a ak av (Even b bk bv c)) k v r -> Even (Even a ak av b) bk bv (Even c k v r)
    | _ -> t

grow_right : Map k v -> Map k v
grow_right t =
  match t with
    | RightHigh l k v (RightHigh a rk rv b) -> Even (Even l k v a) rk rv b
    | RightHigh l k v (LeftHigh (RightHigh a bk bv b) rk rv c) -> Even (LeftHigh l k v a) bk bv (Even b rk rv c)
    | RightHigh l k v (LeftHigh (LeftHigh a bk bv b) rk rv c) -> Even (Even l k v a) bk bv (RightHigh b rk rv c)
    | RightHigh l k v (LeftHigh (Even a bk bv b) rk rv c) -> Even (Even l k v a) bk bv (Even b rk rv c)
    | _ -> t

-- 結果と、部分木の高さが増えたか
ins : Ord k => k -> (Option v -> <e> v) -> Map k v -> <e> (Map k v, Bool)
ins key f t =
  match t with
    | Empty -> (Even Empty key (f None) Empty, True)
    | LeftHigh l k v r ->
      match compare key k with
        | LT ->
          let (l2, grew) = ins key f l
          if grew then (grow_left (LeftHigh l2 k v r), False) else (LeftHigh l2 k v r, False)
        | GT ->
          let (r2, grew) = ins key f r
          if grew then (Even l k v r2, False) else (LeftHigh l k v r2, False)
        | EQ -> (LeftHigh l k (f (Some v)) r, False)
    | Even l k v r ->
      match compare key k with
        | LT ->
          let (l2, grew) = ins key f l
          if grew then (LeftHigh l2 k v r, True) else (Even l2 k v r, False)
        | GT ->
          let (r2, grew) = ins key f r
          if grew then (RightHigh l k v r2, True) else (Even l k v r2, False)
        | EQ -> (Even l k (f (Some v)) r, False)
    | RightHigh l k v r ->
      match compare key k with
        | LT ->
          let (l2, grew) = ins key f l
          if grew then (Even l2 k v r, False) else (RightHigh l2 k v r, False)
        | GT ->
          let (r2, grew) = ins key f r
          if grew then (grow_right (RightHigh l k v r2), False) else (RightHigh l k v r2, False)
        | EQ -> (RightHigh l k (f (Some v)) r, False)

-- `key` の値を `f` で置き換える。なければ `f None` を入れる
pub upsert : Ord k => k -> (Option v -> <e> v) -> Map k v -> <e> Map k v
upsert key f m =
  let (t, _) = ins key f m
  t

pub insert : Ord k => k -> v -> Map k v -> Map k v
insert key value m = upsert key (fn _ -> value) m

-- 左の部分木が1低くなった節を直す。結果と、節の高さが減ったか
shrink_left : Map k v -> (Map k v, Bool)
shrink_left t =
  match t with
    | LeftHigh l k v r -> (Even l k v r, True)
    | Even l k v r -> (RightHigh l k v r, False)
    | RightHigh l k v (Even a rk rv b) -> (LeftHigh (RightHigh l k v a) rk rv b, False)
    | RightHigh l k v (RightHigh a rk rv b) -> (Even (Even l k v a) rk rv b, True)
    | RightHigh l k v (LeftHigh (LeftHigh a bk bv b) rk rv c) -> (Even (Even l k v a) bk bv (RightHigh b rk rv c), True)
    | RightHigh l k v (LeftHigh (RightHigh a bk bv b) rk rv c) -> (Even (LeftHigh l k v a) bk bv (Even b rk rv c), True)
    | RightHigh l k v (LeftHigh (Even a bk bv b) rk rv c) -> (Even (Even l k v a) bk bv (Even b rk rv c), True)
    | _ -> (t, False)

shrink_right : Map k v -> (Map k v, Bool)
shrink_right t =
  match t with
    | RightHigh l k v r -> (Even l k v r, True)
    | Even l k v r -> (LeftHigh l k v r, False)
    | LeftHigh (Even a lk lv b) k v r -> (RightHigh a lk lv (LeftHigh b k v r), False)
    | LeftHigh (LeftHigh a lk lv b) k v r -> (Even a lk lv (Even b k v r), True)
    | LeftHigh (RightHigh a lk lv (LeftHigh b bk bv c)) k v r -> (Even (Even a lk lv b) bk bv (RightHigh c k v r), True)
    | LeftHigh (RightHigh a lk lv (RightHigh b bk bv c)) k v r -> (Even (LeftHigh a lk lv b) bk bv (Even c k v r), True)
    | LeftHigh (RightHigh a lk lv (Even b bk bv c)) k v r -> (Even (Even a lk lv b) bk bv (Even c k v r), True)
    | _ -> (t, False)

-- いちばん小さいキーの節を外す。外したキーと値、残りの木、高さが減ったか。`dk` と `dv` は、届かない空の木の場合に
-- 返す値である
pop_min : Map k v -> k -> v -> (k, v, Map k v, Bool)
pop_min t dk dv =
  match t with
    | Empty -> (dk, dv, Empty, False)
    | LeftHigh l k v r ->
      let (mk, mv, l2, shrunk) = pop_min l dk dv
      if shrunk then
        let (t2, s) = shrink_left (LeftHigh l2 k v r)
        (mk, mv, t2, s)
      else (mk, mv, LeftHigh l2 k v r, False)
    | Even Empty k v r -> (k, v, r, True)
    | Even l k v r ->
      let (mk, mv, l2, shrunk) = pop_min l dk dv
      if shrunk then
        let (t2, s) = shrink_left (Even l2 k v r)
        (mk, mv, t2, s)
      else (mk, mv, Even l2 k v r, False)
    | RightHigh Empty k v r -> (k, v, r, True)
    | RightHigh l k v r ->
      let (mk, mv, l2, shrunk) = pop_min l dk dv
      if shrunk then
        let (t2, s) = shrink_left (RightHigh l2 k v r)
        (mk, mv, t2, s)
      else (mk, mv, RightHigh l2 k v r, False)

-- 結果と、部分木の高さが減ったか
del : Ord k => k -> Map k v -> (Map k v, Bool)
del key t =
  match t with
    | Empty -> (Empty, False)
    | LeftHigh l k v r ->
      match compare key k with
        | LT ->
          let (l2, s) = del key l
          if s then shrink_left (LeftHigh l2 k v r) else (LeftHigh l2 k v r, False)
        | GT ->
          let (r2, s) = del key r
          if s then shrink_right (LeftHigh l k v r2) else (LeftHigh l k v r2, False)
        -- 左が高いので、左は空でない。右が空なら左の1節だけが残る
        | EQ ->
          match r with
            | Empty -> (l, True)
            | _ ->
              let (mk, mv, r2, s) = pop_min r k v
              if s then shrink_right (LeftHigh l mk mv r2) else (LeftHigh l mk mv r2, False)
    | Even l k v r ->
      match compare key k with
        | LT ->
          let (l2, s) = del key l
          if s then shrink_left (Even l2 k v r) else (Even l2 k v r, False)
        | GT ->
          let (r2, s) = del key r
          if s then shrink_right (Even l k v r2) else (Even l k v r2, False)
        | EQ ->
          match r with
            | Empty -> (l, True)
            | _ ->
              let (mk, mv, r2, s) = pop_min r k v
              if s then shrink_right (Even l mk mv r2) else (Even l mk mv r2, False)
    | RightHigh l k v r ->
      match compare key k with
        | LT ->
          let (l2, s) = del key l
          if s then shrink_left (RightHigh l2 k v r) else (RightHigh l2 k v r, False)
        | GT ->
          let (r2, s) = del key r
          if s then shrink_right (RightHigh l k v r2) else (RightHigh l k v r2, False)
        | EQ ->
          let (mk, mv, r2, s) = pop_min r k v
          if s then shrink_right (RightHigh l mk mv r2) else (RightHigh l mk mv r2, False)

pub delete : Ord k => k -> Map k v -> Map k v
delete key m =
  let (t, _) = del key m
  t

pub lookup : Ord k => k -> Map k v -> Option v
lookup key m =
  match m with
    | Empty -> None
    | LeftHigh l k v r ->
      match compare key k with
        | LT -> lookup key l
        | GT -> lookup key r
        | EQ -> Some v
    | Even l k v r ->
      match compare key k with
        | LT -> lookup key l
        | GT -> lookup key r
        | EQ -> Some v
    | RightHigh l k v r ->
      match compare key k with
        | LT -> lookup key l
        | GT -> lookup key r
        | EQ -> Some v

pub member : Ord k => k -> Map k v -> Bool
member key m =
  match lookup key m with
    | Some _ -> True
    | None -> False

-- キーの昇順にたたむ
pub fold : (b -> k -> v -> <e> b) -> b -> Map k v -> <e> b
fold f acc m =
  match m with
    | Empty -> acc
    | LeftHigh l k v r -> fold f (f (fold f acc l) k v) r
    | Even l k v r -> fold f (f (fold f acc l) k v) r
    | RightHigh l k v r -> fold f (f (fold f acc l) k v) r

pub size : Map k v -> <Panic> Int
size m = fold (fn n _ _ -> n + 1) 0 m

-- キーの昇順の組の列
pub to_list : Map k v -> List (k, v)
to_list m =
  let go t acc =
    match t with
      | Empty -> acc
      | LeftHigh l k v r -> go l ((k, v) :: go r acc)
      | Even l k v r -> go l ((k, v) :: go r acc)
      | RightHigh l k v r -> go l ((k, v) :: go r acc)
  go m Nil

-- 同じキーは後の組が勝つ
pub from_list : Ord k => List (k, v) -> Map k v
from_list xs = List.fold (fn m (k, v) -> insert k v m) empty xs
