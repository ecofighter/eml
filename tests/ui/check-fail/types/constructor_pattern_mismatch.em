-- E2001: a constructor pattern of another data type than the matched value.
data Option a =
  | None
  | Some a

data List a =
  | Nil
  | Cons a (List a)

first : List Int -> Int
first xs = match xs with
  | Some x -> x
  | _ -> 0

main : Unit -> <IO> Unit
main () = println (show (first Nil))
