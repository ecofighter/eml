-- E4002 for several equations: the signature is primary and every equation is secondary.
data Option a =
  | None
  | Some a

first : Option Int -> Option Int -> Int
first (Some x) _ = x
first None (Some y) = y

main : Unit -> <IO> Unit
main () = println (show (first None (Some 1)))
