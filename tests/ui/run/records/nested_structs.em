-- 構造体のフィールドが構造体である型。`Particle` は `Vec2` を2つと `Int` を持つ 5 語の構造体で、開くと葉が深さ優先に
-- 並ぶ。入れ子のタプル `(Vec2, (Int, Vec2))` も同じである
-- (docs/spec/lir.md の「構造体の形」)。
data Vec2 =
  | Vec2 Float Float

data Particle =
  | Particle Vec2 Vec2 Int

step : Particle -> <Panic> Particle
step (Particle (Vec2 x y) (Vec2 vx vy) n) =
  Particle (Vec2 (x + vx) (y + vy)) (Vec2 vx (vy - 1.0)) (n + 1)

run : Int -> Particle -> <Panic> Particle
run k p = if k == 0 then p else run (k - 1) (step p)

rotate : (Vec2, (Int, Vec2)) -> <Panic> (Vec2, (Int, Vec2))
rotate (a, (n, b)) = (b, (n + 1, a))

spin : Int -> (Vec2, (Int, Vec2)) -> <Panic> (Vec2, (Int, Vec2))
spin k t = if k == 0 then t else spin (k - 1) (rotate t)

main : Unit -> <IO, Panic> Unit
main () =
  let Particle (Vec2 x y) v n = run 5 (Particle (Vec2 0.0 0.0) (Vec2 1.0 3.0) 0)
  let Vec2 vx vy = v
  println (show x ++ " " ++ show y ++ " " ++ show vx ++ " " ++ show vy ++ " " ++ show n)
  let (Vec2 a b, (m, Vec2 c d)) = spin 3 (Vec2 1.0 2.0, (0, Vec2 3.0 4.0))
  println (show a ++ " " ++ show b ++ " " ++ show m ++ " " ++ show c ++ " " ++ show d)
