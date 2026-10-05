-- E1016: a constructor pattern with more arguments than the constructor has fields.
data Option a = | None | Some a

first : Option Int -> Int
first (Some x y) = x

main : Unit -> <IO> Unit
main () = ()
