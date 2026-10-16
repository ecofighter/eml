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

pub length : List a -> Int
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
