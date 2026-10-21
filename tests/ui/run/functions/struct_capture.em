-- 構造体を捕まえる閉包。捕獲の位置は型どおりなので、`Vec2` は葉のまま閉包に入る。閉包をリストに入れて後で呼ぶ形と、
-- 構造体を受ける関数の部分適用の形と、構造体を入れ子に持つ組を捕まえる形を通す
-- (docs/spec/lir.md の「関数の ABI」)。
data Vec2 =
  | Vec2 Float Float

shift : Vec2 -> Float -> <Panic> Vec2
shift (Vec2 x y) d = Vec2 (x + d) (y - d)

area : Vec2 -> <Panic> Float
area (Vec2 a b) = a * b

offsets : List Vec2 -> List (Float -> <Panic> Float)
offsets vs = match vs with
  | Nil -> Nil
  | v :: rest -> (fn k -> area (shift v k)) :: offsets rest

scaled : (Vec2, Int) -> Int -> <Panic> Float
scaled (Vec2 x _, m) n = x * Float.of_int (n + m)

main : Unit -> <IO, Panic> Unit
main () =
  let fs = offsets [Vec2 1.0 2.0, Vec2 3.0 4.0]
  println (show (List.map (fn f -> f 0.5) fs))
  let moved = shift (Vec2 10.0 20.0)
  let Vec2 a b = moved 2.5
  println (show a ++ " " ++ show b)
  let pair = (Vec2 1.5 2.5, 7)
  let pick = fn n -> scaled pair n
  println (show (pick 1 + pick 2))
