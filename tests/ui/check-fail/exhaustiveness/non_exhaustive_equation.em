-- E4002: the only equation of `first` does not accept `Nil`.
data List a = | Nil | Cons a (List a)

first : List Int -> Int
first (Cons x _) = x

main : Unit -> <IO> Unit
main () = println (show (first (Cons 1 Nil)))
