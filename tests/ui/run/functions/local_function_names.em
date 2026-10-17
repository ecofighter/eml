-- 名前が `boxed`、`k0`、`lambda0` のローカル関数を、直接呼ぶ形と値として渡す形で使う。持ち上げた関数の名前が、生成した
-- 関数 (`f$boxed`、`f$k0`、`f$lambda0`) と重ならないことを確かめる。
-- (docs/spec/core-ir.md の「ローカル関数の持ち上げ」)
effect Ask where
  ask : Unit -> Int

apply : (Int -> Int) -> Int -> Int
apply f x = f x

run : Unit -> <IO> Int
run () =
  let boxed n = n + 1
  let k0 n = n * 2
  let lambda0 n = n - 3
  let direct = boxed (k0 (lambda0 10))
  let as_values = apply boxed (apply k0 (apply lambda0 10))
  let h = handle boxed (ask ()) + k0 1 with
    | ctl ask () k -> k (lambda0 5)
  let l = (fn x -> lambda0 x) 4
  direct + as_values + h + l

main : Unit -> <IO> Unit
main () =
  println (show (run ()))
