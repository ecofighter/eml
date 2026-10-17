-- 引いて使うジェネレータ (docs/spec/stdlib.md)。`next` を呼ぶまで何も動かさない。
-- `Iter` は `once` の関数を持つので `Lin` である。途中でやめるときは `drop` で捨てる。
import Yield (Yield(..))

pub data Iter <e> a = | Iter (once Unit -> <e> Option (a, Iter <e> a))

pub next : Iter <e> a -> <e> Option (a, Iter <e> a)
next (Iter pull) = pull ()

-- 継続 `k` をそのまま次の `Iter` にする。生産者が持つ `File` は、継続を捨てるときに閉じる
pub generate : (Unit -> <Yield a | e> Unit) -> Iter <e> a
generate producer =
  Iter (fn () ->
    handle producer () with
      | ctl yield x k -> Some (x, Iter k)
      | return _ -> None)

pub yield_all : Iter <e> a -> <Yield a | e> Unit
yield_all it =
  match next it with
    | None -> ()
    | Some (x, rest) ->
      yield x
      yield_all rest

pub from_list : List a -> Iter <e> a
from_list xs =
  Iter (fn () ->
    match xs with
      | Nil -> None
      | x :: rest -> Some (x, from_list rest))

pub map : (a -> <e> b) -> Iter <e> a -> Iter <e> b
map f it =
  Iter (fn () ->
    match next it with
      | None -> None
      | Some (x, rest) -> Some (f x, map f rest))

pub filter : (a -> <e> Bool) -> Iter <e> a -> Iter <e> a
filter p it =
  Iter (fn () ->
    match next it with
      | None -> None
      | Some (x, rest) ->
        if p x then Some (x, filter p rest) else next (filter p rest))

pub filter_map : (a -> <e> Option b) -> Iter <e> a -> Iter <e> b
filter_map f it =
  Iter (fn () ->
    match next it with
      | None -> None
      | Some (x, rest) ->
        match f x with
          | Some y -> Some (y, filter_map f rest)
          | None -> next (filter_map f rest))

-- n 個を渡した後の `next` で残りを捨てる。`File` が閉じるのはそのときになる
pub take : Int -> Iter <e> a -> Iter <e> a
take n it =
  Iter (fn () ->
    if n <= 0 then
      drop it
      None
    else
      match next it with
        | None -> None
        | Some (x, rest) -> Some (x, take (n - 1) rest))

-- どちらかが終わったら、もう一方の残りを捨てる
pub zip : Iter <e> a -> Iter <e> b -> Iter <e> (a, b)
zip left right =
  Iter (fn () ->
    match next left with
      | None ->
        drop right
        None
      | Some (x, left_rest) ->
        match next right with
          | None ->
            drop left_rest
            None
          | Some (y, right_rest) -> Some ((x, y), zip left_rest right_rest))

pub enumerate : Int -> Iter <e> a -> Iter <e> (Int, a)
enumerate start it =
  Iter (fn () ->
    match next it with
      | None -> None
      | Some (x, rest) -> Some ((start, x), enumerate (start + 1) rest))

pub each : (a -> <e> Unit) -> Iter <e> a -> <e> Unit
each f it =
  match next it with
    | None -> ()
    | Some (x, rest) ->
      f x
      each f rest

pub fold : (b -> a -> <e> b) -> b -> Iter <e> a -> <e> b
fold f acc it =
  match next it with
    | None -> acc
    | Some (x, rest) -> fold f (f acc x) rest

-- 先頭に積んでいくので、最後に逆順に戻す
pub to_list : Iter <e> a -> <e> List a
to_list it = reverse_onto Nil (fold (fn acc x -> x :: acc) Nil it)

reverse_onto : List a -> List a -> List a
reverse_onto acc Nil = acc
reverse_onto acc (x :: rest) = reverse_onto (x :: acc) rest
