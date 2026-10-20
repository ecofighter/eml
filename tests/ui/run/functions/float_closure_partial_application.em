-- `Float` を捕まえた閉包を部分適用し、データに通してから残りの引数で呼ぶ。捕まえた `Float` と部分適用の `Float` は
-- 箱に入れず、呼ばれる関数の引数の Repr で閉包に持つ。`over` は引数を1つ取る関数の値に2つの引数を渡すので、余った
-- 引数を預けてから、返った閉包に渡す (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「VM」)。
data Box a = | Box a

hide : Int -> a -> <Panic> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x

main : Unit -> <IO, Panic> Unit
main () =
  let k = 1.5
  let f = fn a b c -> a * k + b - c
  let Box g = hide 0 (f 2.0)
  println (show (g 1.0 0.25))
  let Box h = hide 0 (g (-0.5))
  println (show (h 0.25))
  let over = fn x -> fn y -> x * y + k
  let Box o = hide 0 over
  println (show (o 2.0 3.0))
