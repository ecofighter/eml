-- Nested constructor patterns, wildcards and a fallback arm share one decision tree.
data Option a =
  | None
  | Some a

data List a =
  | Nil
  | Cons a (List a)

first_two : List (Option Int) -> String
first_two xs = match xs with
  | Cons (Some a) (Cons (Some b) _) -> "both " ++ show_int (a + b)
  | Cons None (Cons (Some b) _) -> "second " ++ show_int b
  | Cons _ _ -> "other"
  | Nil -> "empty"

main : Unit -> <IO> Unit
main () =
  println (first_two (Cons (Some 1) (Cons (Some 2) Nil)))
  println (first_two (Cons None (Cons (Some 3) (Cons None Nil))))
  println (first_two (Cons (Some 4) (Cons None Nil)))
  println (first_two (Cons None Nil))
  println (first_two Nil)
