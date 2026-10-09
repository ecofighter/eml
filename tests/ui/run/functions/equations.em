-- Functions defined by several equations, over literals, constructors and several arguments.
data List a =
  | Nil
  | Cons a (List a)

len : List a -> Int
len Nil = 0
len (Cons _ rest) = 1 + len rest

describe : Int -> String
describe 0 = "zero"
describe 1 = "one"
describe _ = "many"

zip_sum : List Int -> List Int -> Int
zip_sum (Cons x xs) (Cons y ys) = x + y + zip_sum xs ys
zip_sum _ _ = 0

main : Unit -> <IO> Unit
main () =
  let xs = Cons 1 (Cons 2 (Cons 3 Nil))
  println (show (len xs))
  println (describe 0)
  println (describe 1)
  println (describe 5)
  println (show (zip_sum (Cons 1 (Cons 2 Nil)) (Cons 10 (Cons 20 (Cons 30 Nil)))))
