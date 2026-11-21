-- 分割の形は長さと粒度だけで決まる。`grain`個の添字ごとの塊、`ceil(sqrt(chunks))`個ずつの塊の群、
-- どちらの段でも左に入れ子になる結合である。括弧を書く結合で形が見える (docs/spec/stdlib.mdの「`Std.Par`」)
import Std.Par

shape : Int -> Int -> Int -> <Panic> String
shape grain lo hi = Par.reduce_range_grain grain (fn l r -> "(\{l} \{r})") "empty" lo hi (fn i -> show i)

data Span =
  | Run Int Int
  | Fork Span Span

-- 塊の中の結合は連なりを延ばす。塊の境目 (1024の倍数の添字) での結合と、結合済みの2つの部分の結合は分岐を作る
join : Span -> Span -> <Panic> Span
join l r =
  match (l, r) with
    | (Run a b, Run c d) -> if c == d && c % 1024 != 0 then Run a d else Fork l r
    | _ -> Fork l r

render : Span -> String
render s =
  match s with
    | Run a b -> "\{show a}..\{show b}"
    | Fork l r -> "(\{render l} \{render r})"

main : Unit -> <IO, Panic> Unit
main () =
  println (shape 2 0 0)
  println (shape 2 0 1)
  println (shape 2 0 7)
  println (shape 2 10 31)
  println (shape 1 0 5)
  println (render (Par.reduce_range join (Run 0 0) 0 5000 (fn i -> Run i i)))
  println (show (try_panic (fn () -> shape 0 0 3)))
