-- A list of 100000 cells is summed by a non-tail recursion, and another one is dropped whole when only its
-- head is read. Freeing the long tail must not recurse in Rust.
data List a =
  | Nil
  | Cons a (List a)

build : Int -> List Int -> List Int
build n acc = if n == 0 then acc else build (n - 1) (Cons n acc)

total : List Int -> Int
total xs = match xs with
  | Nil -> 0
  | Cons x rest -> x + total rest

first_or_zero : List Int -> Int
first_or_zero xs = match xs with
  | Nil -> 0
  | Cons x _ -> x

main : Unit -> <IO> Unit
main () =
  println (show_int (total (build 100000 Nil)))
  println (show_int (first_or_zero (build 100000 Nil)))
