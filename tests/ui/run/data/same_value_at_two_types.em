-- 型引数だけが違う値 `P (1, 2)` を、1つの葉が2回渡す。コンストラクタとフィールドのアトムが同じなので、葉は
-- `con` を1回だけ作り、同じ値を2つの引数に渡す。`keep` は片方を返し、もう片方を分解して捨てる。フィールドは
-- ヒープのタプルなので、参照の数が正しくないと debug_heap が見つける。
data P a =
  | P (Int, Int)

keep : P Int -> P String -> P Int
keep x y = match y with
  | P (c, _) -> x

main : Unit -> <IO> Unit
main () =
  let r = match (P (1, 2), P (1, 2)) with
    | (x, y) -> keep x y
  match r with
    | P (a, b) ->
      println (show a)
      println (show b)
