-- `-0.0` と NaN を、型から決める呼び出しの規約の道に通しても、直接渡したときと同じに書く。道は、一様な `apply` の引数と結果の `box` と
-- `unbox` (`pass@[_]` を関数の値で呼ぶ)、閉包の生の捕獲、構造体の葉、複数の値の戻り (`swap` が返す `Vec2`) である。
-- `compare` は全順序で、`-0.0` を `0.0` より小さいとする。NaN のペイロードを見せる eml の関数はないので、確かめるのは
-- `-0.0` と NaN の符号 (`compare` の結果) と、NaN であることだけである
-- (docs/spec/lir.md の「関数の ABI」)。
data Vec2 =
  | Vec2 Float Float

data Box a = | Box a

hide : Int -> a -> <Panic> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x

pass : Int -> a -> <Panic> a
pass n x = if n == 0 then x else
  let (y, _) = pass (n - 1) (x, n)
  y

swap : Int -> Vec2 -> <Panic> Vec2
swap n v = if n == 0 then v else
  let Vec2 a b = v
  swap (n - 1) (Vec2 b a)

describe : Float -> <Panic> String
describe x = show x ++ " " ++ show (compare x 0.0) ++ " " ++ show (Float.is_nan x)

report : String -> Float -> Float -> <IO, Panic> Unit
report label a b = println (label ++ ": " ++ describe a ++ " / " ++ describe b)

main : Unit -> <IO, Panic> Unit
main () =
  let z = -0.0
  let n = Float.nan
  report "direct" z n
  let Box through = hide 0 (pass 2)
  report "converted" (through z) (through n)
  let Box captured = hide 0 (fn u -> (z, n))
  let (cz, cn) = captured ()
  report "captured" cz cn
  let Vec2 sz sn = swap 2 (Vec2 z n)
  report "struct leaves" sz sn
  let Vec2 rn rz = swap 3 (Vec2 z n)
  report "returned" rz rn
