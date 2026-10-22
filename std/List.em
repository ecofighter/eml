-- リストの関数 (docs/spec/stdlib.md)。どれも末尾再帰で書き、逆順に積んでから `reverse` する。長いリストでも VM の
-- 深さの上限に当たらないようにするためである。関数を受ける関数は、要素の順に関数を呼ぶ。

pub reverse : List a -> List a
reverse xs = reverse_onto Nil xs

-- `xs` を逆順に `acc` の前に積む
reverse_onto : List a -> List a -> List a
reverse_onto acc Nil = acc
reverse_onto acc (x :: rest) = reverse_onto (x :: acc) rest

pub append : List a -> List a -> List a
append xs ys = reverse_onto ys (reverse xs)

pub length : List a -> <Panic> Int
length xs =
  let go n ys =
    match ys with
      | Nil -> n
      | _ :: rest -> go (n + 1) rest
  go 0 xs

pub map : (a -> <e> b) -> List a -> <e> List b
map f xs =
  let go acc ys =
    match ys with
      | Nil -> reverse acc
      | x :: rest -> go (f x :: acc) rest
  go Nil xs

pub filter : (a -> <e> Bool) -> List a -> <e> List a
filter p xs =
  let go acc ys =
    match ys with
      | Nil -> reverse acc
      | x :: rest -> if p x then go (x :: acc) rest else go acc rest
  go Nil xs

pub filter_map : (a -> <e> Option b) -> List a -> <e> List b
filter_map f xs =
  let go acc ys =
    match ys with
      | Nil -> reverse acc
      | x :: rest ->
        match f x with
          | Some y -> go (y :: acc) rest
          | None -> go acc rest
  go Nil xs

-- 左からたたむ。`fold f init [x1, x2]` は `f (f init x1) x2` である
pub fold : (b -> a -> <e> b) -> b -> List a -> <e> b
fold f acc Nil = acc
fold f acc (x :: rest) = fold f (f acc x) rest

pub each : (a -> <e> Unit) -> List a -> <e> Unit
each f Nil = ()
each f (x :: rest) =
  f x
  each f rest

-- 先頭の `n` 個。`n` が 0 以下なら `Nil`、長さより大きければ全体
pub take : Int -> List a -> <Panic> List a
take n xs =
  let go i acc rest =
    match rest with
      | x :: more -> if i > 0 then go (i - 1) (x :: acc) more else reverse acc
      | Nil -> reverse acc
  go n Nil xs

-- 先頭の `n` 個と残り
pub split_at : Int -> List a -> <Panic> (List a, List a)
split_at n xs =
  let go i acc rest =
    match rest with
      | x :: more -> if i > 0 then go (i - 1) (x :: acc) more else (reverse acc, rest)
      | Nil -> (reverse acc, Nil)
  go n Nil xs

-- 0 から数えた位置 `i` の要素。範囲の外なら `None`
pub nth : Int -> List a -> <Panic> Option a
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else if i < 0 then None else nth (i - 1) rest

-- 短いほうの長さで止める
pub zip : List a -> List b -> List (a, b)
zip xs ys =
  let go acc l r =
    match (l, r) with
      | (x :: ls, y :: rs) -> go ((x, y) :: acc) ls rs
      | _ -> reverse acc
  go Nil xs ys

pub index_of : Eq a => a -> List a -> <Panic> Option Int
index_of x xs =
  let go i rest =
    match rest with
      | Nil -> None
      | y :: more -> if x == y then Some i else go (i + 1) more
  go 0 xs

pub contains : Eq a => a -> List a -> Bool
contains x Nil = False
contains x (y :: rest) = if x == y then True else contains x rest

-- 組のリストで、最初にキーが等しい組の値
pub lookup : Eq k => k -> List (k, v) -> Option v
lookup key Nil = None
lookup key ((k, v) :: rest) = if k == key then Some v else lookup key rest
