-- A recursive list: length and sum walk the structure, and every cell is freed.
data List a =
  | Nil
  | Cons a (List a)

length : List a -> Int
length xs = match xs with
  | Nil -> 0
  | Cons _ rest -> 1 + length rest

sum : List Int -> Int
sum xs = match xs with
  | Nil -> 0
  | Cons x rest -> x + sum rest

range : Int -> Int -> List Int
range lo hi = if lo > hi then Nil else Cons lo (range (lo + 1) hi)

main : Unit -> <IO> Unit
main () =
  let xs = range 1 10
  println (show (length xs))
  println (show (sum xs))
  println (show (sum (range 1 1000)))
