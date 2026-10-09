-- 型変数のフィールドに `Int` を入れるリストと、多相な高階関数。
data List a =
  | Nil
  | Cons a (List a)

range : Int -> Int -> List Int
range lo hi = if lo > hi then Nil else Cons lo (range (lo + 1) hi)

map : (a -> b) -> List a -> List b
map f xs = match xs with
  | Nil -> Nil
  | Cons x rest -> Cons (f x) (map f rest)

foldl : (b -> a -> b) -> b -> List a -> b
foldl f acc xs = match xs with
  | Nil -> acc
  | Cons x rest -> foldl f (f acc x) rest

main : Unit -> <IO> Unit
main () =
  let doubled = map (fn x -> x * 2) (range 1 100000)
  println (show_int (foldl (fn acc x -> acc + x) 0 doubled))
