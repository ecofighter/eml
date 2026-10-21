-- 構造体の型を宣言したフィールド。コンストラクタが2つある `Shape` の `Ball` は構造体にならず、フィールドの `Vec3` は
-- 配置では一様な位置 (`tobj`) で、箱に入れて持つ。構造体の `Body` のフィールドの `Vec3` は、構造体の入れ子の葉になる
-- (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「箱に入れる位置」)。
data Vec3 =
  | Vec3 Float Float Float

data Shape =
  | Ball Vec3 Float
  | Empty

data Body =
  | Body { pos : Vec3, mass : Float }

volume : Shape -> <Panic> Float
volume shape = match shape with
  | Ball _ r -> 4.0 * r * r * r
  | Empty -> 0.0

centre : Shape -> <Panic> Vec3
centre shape = match shape with
  | Ball c _ -> c
  | Empty -> Vec3 0.0 0.0 0.0

moved : Body -> Vec3 -> <Panic> Body
moved body (Vec3 dx dy dz) =
  let Vec3 x y z = body.pos
  { body | pos = Vec3 (x + dx) (y + dy) (z + dz) }

main : Unit -> <IO, Panic> Unit
main () =
  let shapes = [Ball (Vec3 1.0 2.0 3.0) 0.5, Empty, Ball (Vec3 (-1.0) 0.0 1.0) 2.0]
  println (show (List.fold (fn acc s -> acc + volume s) 0.0 shapes))
  let Vec3 x y z = centre (Ball (Vec3 4.0 5.0 6.0) 1.0)
  println (show (x + y + z))
  let body = moved (Body { pos = Vec3 1.0 1.0 1.0, mass = 2.5 }) (Vec3 0.5 0.25 0.125)
  let Vec3 bx by bz = body.pos
  println (show bx ++ " " ++ show by ++ " " ++ show bz ++ " " ++ show body.mass)
