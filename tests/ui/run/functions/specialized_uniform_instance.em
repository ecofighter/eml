-- 多相再帰の一様な instance の中で、既知の閉包を呼ぶ。`tobj` の引数と閉包の関数の引数の Repr の違いは、box の挿入が
-- 直す
import List

data Nested a =
  | Flat a
  | Nest (Nested (List a))

map_nested : (a -> <Panic> b) -> Nested a -> <Panic> Nested b
map_nested f n = match n with
  | Flat x -> Flat (f x)
  | Nest inner -> Nest (map_nested (List.map f) inner)

total : (a -> <Panic> Int) -> Nested a -> <Panic> Int
total g n = match n with
  | Flat x -> g x
  | Nest inner -> total (fn xs -> List.fold (fn acc x -> acc + g x) 0 xs) inner

main : Unit -> <IO, Panic> Unit
main () =
  let n = Nest (Nest (Flat [[1, 2], [3]]))
  println (show (total (fn x -> x) (map_nested (fn x -> x + 1) n)))
