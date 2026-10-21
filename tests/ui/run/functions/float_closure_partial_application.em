-- `Float` を捕まえた閉包を部分適用し、データに通してから残りの引数で呼ぶ。捕まえた `k` は箱に入れず、呼ばれる関数の
-- 引数の Repr で閉包に持つ。部分適用で足す `Float` は捕獲より後ろの一様な位置なので、箱に入れて持つ。`over` は引数を
-- 1つ取る関数の値に2つの引数を渡すので、余った引数を預けてから、返った閉包に渡す
-- (docs/spec/lir.md の「関数の ABI」、docs/spec/vm.md の「呼び出しと戻り」)。
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
