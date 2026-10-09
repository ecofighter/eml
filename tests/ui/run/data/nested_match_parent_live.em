-- 入れ子のパターンの内側に、外側の値 `w` を使う枝がある。内側のリストを分解する時点で、外側のセルはまだ
-- 生きている。
data List a =
  | Nil
  | Cons a (List a)

length : List a -> Int
length xs = match xs with
  | Nil -> 0
  | Cons _ rest -> 1 + length rest

describe : List (List String) -> String
describe w = match w with
  | Cons (Cons a _) _ -> a
  | Cons Nil _ -> show (length w)
  | Nil -> "empty"

main : Unit -> <IO> Unit
main () =
  println (describe (Cons (Cons "a" (Cons "b" Nil)) Nil))
  println (describe (Cons Nil (Cons Nil Nil)))
  println (describe Nil)
