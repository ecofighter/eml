-- トップレベルの関数、extern の関数、コンストラクタ、クラスのメソッドの参照は何も捕まえないので、同じものを
-- `once` の位置にも `->` の位置にも置ける (docs/spec/types.md の「トップレベルの関数の参照」)。
data Once <e> a b =
  | Once (once a -> <e> b)

data Many <e> a b =
  | Many (a -> <e> b)

apply_once : Once <e> a b -> a -> <e> b
apply_once o x =
  match o with
    | Once f -> f x

apply_twice : Many <e> a b -> a -> a -> <e> (b, b)
apply_twice m x y =
  match m with
    | Many f -> (f x, f y)

run_once : (once Int -> <Panic> Int) -> Int -> <Panic> Int
run_once f x = f x

inc : Int -> <Panic> Int
inc x = x + 1

main : Unit -> <IO, Panic> Unit
main () =
  println (show (apply_once (Once inc) 1))
  println (show (apply_twice (Many inc) 1 2))
  println (show (run_once inc 10))
  println (show (apply_once (Once String.byte_length) "hello"))
  println (show (apply_twice (Many String.byte_length) "a" "bc"))
  apply_once (Once println) "println in a once field"
  let _ = apply_twice (Many println) "println in a field, 1" "println in a field, 2"
  println (show (apply_once (Once Some) 3))
  println (show (apply_twice (Many Some) 3 4))
  println (apply_once (Once show) 42)
  println (show (apply_twice (Many show) 5 6))
