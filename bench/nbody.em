-- nbody (S14d の計測)。Benchmarks Game の5つの天体を引数の回数 (既定は 1000) だけ進め、進める前と後のエネルギーを書く。
-- ベクトルは `Vec3` の値で書く。今は `Vec3` を作るたびに箱を確保し、S14e の構造体の表現がその確保を消す
-- (docs/implementation/benchmarks.md の「S14d」)。bench/numeric/nbody.py は、同じ順で浮動小数の演算を行う Python の版である
data Vec3 =
  | Vec3 Float Float Float

add : Vec3 -> Vec3 -> <Panic> Vec3
add (Vec3 a b c) (Vec3 x y z) = Vec3 (a + x) (b + y) (c + z)

sub : Vec3 -> Vec3 -> <Panic> Vec3
sub (Vec3 a b c) (Vec3 x y z) = Vec3 (a - x) (b - y) (c - z)

scale : Vec3 -> Float -> <Panic> Vec3
scale (Vec3 x y z) k = Vec3 (x * k) (y * k) (z * k)

dot : Vec3 -> Vec3 -> <Panic> Float
dot (Vec3 a b c) (Vec3 x y z) = a * x + b * y + c * z

-- `4π²`。トップレベルの値の本体は算術を書けないので、リテラルで書く (docs/spec/declarations.md の「シグネチャと等式」)
solar_mass : Float
solar_mass = 39.47841760435743

days_per_year : Float
days_per_year = 365.24

dt : Float
dt = 0.01

positions : Unit -> Array Vec3
positions () =
  Array.from_list
    [ Vec3 0.0 0.0 0.0
    , Vec3 4.84143144246472090e+00 (-1.16032004402742839e+00) (-1.03622044471123109e-01)
    , Vec3 8.34336671824457987e+00 4.12479856412430479e+00 (-4.03523417114321381e-01)
    , Vec3 1.28943695621391310e+01 (-1.51111514016986312e+01) (-2.23307578892655734e-01)
    , Vec3 1.53796971148509165e+01 (-2.59193146099879641e+01) 1.79258772950371181e-01
    ]

velocities : Unit -> <Panic> Array Vec3
velocities () =
  Array.from_list
    [ scale (Vec3 0.0 0.0 0.0) days_per_year
    , scale (Vec3 1.66007664274403694e-03 7.69901118419740425e-03 (-6.90460016972063023e-05)) days_per_year
    , scale (Vec3 (-2.76742510726862411e-03) 4.99852801234917238e-03 2.30417297573763929e-05) days_per_year
    , scale (Vec3 2.96460137564761618e-03 2.37847173959480950e-03 (-2.96589568540237556e-05)) days_per_year
    , scale (Vec3 2.68067772490389322e-03 1.62824170038242295e-03 (-9.51592254519715870e-05)) days_per_year
    ]

masses : Unit -> <Panic> Array Float
masses () =
  Array.from_list
    [ solar_mass
    , 9.54791938424326609e-04 * solar_mass
    , 2.85885980666130812e-04 * solar_mass
    , 4.36624404335156298e-05 * solar_mass
    , 5.15138902046611451e-05 * solar_mass
    ]

-- 全体の運動量が 0 になるように、太陽の速度を決める
offset : Array Vec3 -> Array Float -> <Panic> Array Vec3
offset vel mass =
  var p = Vec3 0.0 0.0 0.0
  for 0 (Array.length mass) (fn i -> p <- add p (scale vel[i] mass[i]))
  let Vec3 px py pz = p
  Array.set vel 0 (Vec3 (0.0 - px / solar_mass) (0.0 - py / solar_mass) (0.0 - pz / solar_mass))

energy : Array Vec3 -> Array Vec3 -> Array Float -> <Panic> Float
energy pos vel mass =
  let n = Array.length mass
  var e = 0.0
  for 0 n (fn i ->
    e <- e + 0.5 * mass[i] * dot vel[i] vel[i]
    for (i + 1) n (fn j ->
      let d = sub pos[i] pos[j]
      e <- e - mass[i] * mass[j] / Float.sqrt (dot d d)))
  e

-- 配列の変数は範囲の最後の式でしか取り出せない (E3008) ので、進める計算と最後の表示を1つの関数に書く
simulate : Int -> Array Vec3 -> Array Vec3 -> Array Float -> <IO, Panic> Unit
simulate steps pos0 vel0 mass =
  let n = Array.length mass
  var pos = Array.thaw pos0
  var vel = Array.thaw vel0
  for 0 steps (fn _ ->
    for 0 n (fn i ->
      for (i + 1) n (fn j ->
        let d = sub pos[i] pos[j]
        let d2 = dot d d
        let mag = dt / (d2 * Float.sqrt d2)
        vel[i] <- sub vel[i] (scale d (mass[j] * mag))
        vel[j] <- add vel[j] (scale d (mass[i] * mag))))
    for 0 n (fn i -> pos[i] <- add pos[i] (scale vel[i] dt)))
  report (MutArray.freeze pos) (MutArray.freeze vel) mass

report : Array Vec3 -> Array Vec3 -> Array Float -> <IO, Panic> Unit
report pos vel mass = println (Float.to_fixed 9 (energy pos vel mass))

main : Unit -> <IO, Panic> Unit
main () =
  let steps = size 1000
  let mass = masses ()
  let pos = positions ()
  let vel = offset (velocities ()) mass
  report pos vel mass
  simulate steps pos vel mass

-- 引数がなければ既定の大きさを使う。`bench/scripts.sh` は大きな大きさを引数で渡す
-- (docs/implementation/benchmarks.md の「スクリプトの計測」)
size : Int -> <IO, Panic> Int
size default =
  match Env.args () with
    | Nil -> default
    | [arg] ->
      match String.to_int arg with
        | Some n -> if n >= 1 then n else usage ()
        | None -> usage ()
    | _ -> usage ()

usage : Unit -> <Panic> Int
usage () = panic "usage: nbody [SIZE]"
