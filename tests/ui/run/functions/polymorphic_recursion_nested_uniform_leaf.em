-- 一様な位置の型変数が入れ子のタプルや data の中にある型は、外側も構造体にしない。呼ぶ側の `(Int, (Float, Int))` と
-- `walk@[_]` の `(Int, (_, Int))` で形が違ってしまうためである
-- (docs/spec/core-ir.md の「構造体にする型」)
data Pair a b = | Pair a b

walk : Int -> (Int, (a, Int)) -> <Panic> Int
walk n p = if n == 0 then 0 else 1 + walk (n - 1) (1, (p, 2))

walk_pair : Int -> (Int, Pair a Int) -> <Panic> Int
walk_pair n p = if n == 0 then 0 else 1 + walk_pair (n - 1) (1, Pair p 2)

main : Unit -> <IO, Panic> Unit
main () =
  println (show (walk 5 (1, (2.5, 3))))
  println (show (walk_pair 5 (1, Pair 2.5 3)))
