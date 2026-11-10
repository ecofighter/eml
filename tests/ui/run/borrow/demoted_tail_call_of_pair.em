-- 2つの`Float`のレコードを返す関数を、新しい文字列を渡して末尾の位置で呼ぶ。呼ばれる関数は文字列を読むだけなので借り、
-- `open`でないので、呼ぶ側は文字列を持ったまま末尾呼び出しを通常の呼び出しに降格する (docs/spec/lir.mdの「借用の推論」)。
-- 呼ばれる関数は再開できる操作も行うので、呼ぶ側のフレームは再開をまたいで文字列を持つ。`span`はoptimizeが展開しない長さにしてある。
import Float
import String

effect Tick where
  tick : Unit -> Int

data Span = | Span Float Float

span : String -> <Tick, Panic> Span
span s =
  let w = Float.of_int (String.byte_length s)
  let t0 = tick ()
  let t1 = tick ()
  let t2 = tick ()
  let t3 = tick ()
  let t4 = tick ()
  let t5 = tick ()
  let t6 = tick ()
  let t7 = tick ()
  let t8 = tick ()
  let t9 = tick ()
  let t10 = tick ()
  let t11 = tick ()
  let t12 = tick ()
  let t13 = tick ()
  let t14 = tick ()
  let t15 = tick ()
  let t16 = tick ()
  let t17 = tick ()
  let t18 = tick ()
  let t19 = tick ()
  let lo = t0 + t1 + t2 + t3 + t4 + t5 + t6 + t7 + t8 + t9
  let hi = t10 + t11 + t12 + t13 + t14 + t15 + t16 + t17 + t18 + t19
  Span (w + Float.of_int lo) (w * Float.of_int hi)

measure : Int -> <Tick, Panic> Span
measure k = span ("item-" ++ show k)

data Box a = | Box a

hide : Int -> a -> <Panic> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x

run : (Int -> <Tick, Panic> Span) -> Int -> <Panic> Span
run body k =
  handle body k from 0 with
    | ctl tick () r st -> r (st + 1) (st + 1)
    | return x _ -> x

main : Unit -> <IO, Panic> Unit
main () =
  let Box body = hide 0 measure
  let Span a b = run body 7
  println (show a ++ " " ++ show b)
  let Span a b = run body 123
  println (show a ++ " " ++ show b)
