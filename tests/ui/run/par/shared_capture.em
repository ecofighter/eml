-- 親が作ったリストと、レコードを捕まえた閉包を、両方の子が読む。親は join の後でもリストを使う。fork は右の子の
-- 閉包から届く物体に共有の印を付ける (docs/spec/vm.md の「`par` の fork と join」)。印を付けないと、右の子の
-- `dup` を検査モードが `WrongOwner` で断る。
import Std.Par

data Scale = | Scale { factor : Int, offset : Int }

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

upto : Int -> List Int -> <Panic> List Int
upto n acc = if n == 0 then acc else upto (n - 1) (n :: acc)

sum : List Int -> <Panic> Int
sum xs = match xs with
  | [] -> 0
  | x :: rest -> x + sum rest

map : (Int -> <Panic> Int) -> List Int -> <Panic> List Int
map f xs = match xs with
  | [] -> []
  | x :: rest -> f x :: map f rest

main : Unit -> <IO, Panic> Unit
main () =
  let xs = upto 20 []
  let s = Scale { factor = 3, offset = 1 }
  let f = fn x -> x * s.factor + s.offset
  let (a, b) = Par.par (fn () -> spin 40 + sum (map f xs)) (fn () -> sum (map f xs))
  println (show (a, b))
  println (show (sum xs))
