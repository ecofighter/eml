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
length xs = length_from 0 xs

length_from : Int -> List a -> Int
length_from n Nil = n
length_from n (_ :: rest) = length_from (n + 1) rest

pub map : (a -> <e> b) -> List a -> <e> List b
map f xs = map_onto f Nil xs

map_onto : (a -> <e> b) -> List b -> List a -> <e> List b
map_onto f acc Nil = reverse acc
map_onto f acc (x :: rest) = map_onto f (f x :: acc) rest

pub filter : (a -> <e> Bool) -> List a -> <e> List a
filter p xs = filter_onto p Nil xs

filter_onto : (a -> <e> Bool) -> List a -> List a -> <e> List a
filter_onto p acc Nil = reverse acc
filter_onto p acc (x :: rest) =
  if p x then filter_onto p (x :: acc) rest else filter_onto p acc rest

pub filter_map : (a -> <e> Option b) -> List a -> <e> List b
filter_map f xs = filter_map_onto f Nil xs

filter_map_onto : (a -> <e> Option b) -> List b -> List a -> <e> List b
filter_map_onto f acc Nil = reverse acc
filter_map_onto f acc (x :: rest) =
  match f x with
    | Some y -> filter_map_onto f (y :: acc) rest
    | None -> filter_map_onto f acc rest

-- 左からたたむ。`fold f init [x1, x2]` は `f (f init x1) x2` である
pub fold : (b -> a -> <e> b) -> b -> List a -> <e> b
fold f acc Nil = acc
fold f acc (x :: rest) = fold f (f acc x) rest

pub each : (a -> <e> Unit) -> List a -> <e> Unit
each f Nil = ()
each f (x :: rest) =
  f x
  each f rest
