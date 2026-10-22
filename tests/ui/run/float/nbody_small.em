-- The Computer Language Benchmarks Game の nbody を 1000 歩動かし、前後のエネルギーを小数点以下 9 桁で書く。
-- 期待値は同じ計算の Python の版 (5 体、`advance 0.01`、n = 1000) の出力で、演算の順も Python の版と同じにした。
-- `** (-1.5)` と `** 0.5` は、Python と同じく `pow` で求める。
import Float
import List

data Body =
  | Body { x : Float, y : Float, z : Float, vx : Float, vy : Float, vz : Float, mass : Float }

solar_mass : Unit -> <Panic> Float
solar_mass () = 4.0 * Float.pi * Float.pi

days_per_year : Float
days_per_year = 365.24

body : Float -> Float -> Float -> Float -> Float -> Float -> Float -> <Panic> Body
body x y z vx vy vz mass =
  Body
    { x
    , y
    , z
    , vx = vx * days_per_year
    , vy = vy * days_per_year
    , vz = vz * days_per_year
    , mass = mass * solar_mass ()
    }

bodies : Unit -> <Panic> List Body
bodies () =
  [ Body { x = 0.0, y = 0.0, z = 0.0, vx = 0.0, vy = 0.0, vz = 0.0, mass = solar_mass () }
  , body 4.84143144246472090e+00 (-1.16032004402742839e+00) (-1.03622044471123109e-01)
      1.66007664274403694e-03 7.69901118419740425e-03 (-6.90460016972063023e-05)
      9.54791938424326609e-04
  , body 8.34336671824457987e+00 4.12479856412430479e+00 (-4.03523417114321381e-01)
      (-2.76742510726862411e-03) 4.99852801234917238e-03 2.30417297573763929e-05
      2.85885980666130812e-04
  , body 1.28943695621391310e+01 (-1.51111514016986312e+01) (-2.23307578892655734e-01)
      2.96460137564761618e-03 2.37847173959480950e-03 (-2.96589568540237556e-05)
      4.36624404335156298e-05
  , body 1.53796971148509165e+01 (-2.59193146099879641e+01) 1.79258772950371181e-01
      2.68067772490389322e-03 1.62824170038242295e-03 (-9.51592254519715870e-05)
      5.15138902046611451e-05
  ]

momentum : Float -> Float -> Float -> List Body -> <Panic> (Float, Float, Float)
momentum px py pz bs =
  match bs with
    | [] -> (px, py, pz)
    | b :: rest -> momentum (px - b.vx * b.mass) (py - b.vy * b.mass) (pz - b.vz * b.mass) rest

-- 太陽の速度を、全体の運動量が 0 になるように決める
offset_momentum : List Body -> <Panic> List Body
offset_momentum bs =
  let (px, py, pz) = momentum 0.0 0.0 0.0 bs
  match bs with
    | [] -> []
    | sun :: rest -> { sun | vx = px / sun.mass, vy = py / sun.mass, vz = pz / sun.mass } :: rest

-- `b` と `rest` の各体の組の速度を、Python の版と同じ順に変える
interact : Float -> Body -> List Body -> <Panic> (Body, List Body)
interact dt b rest =
  match rest with
    | [] -> (b, [])
    | o :: more ->
      let dx = b.x - o.x
      let dy = b.y - o.y
      let dz = b.z - o.z
      let mag = dt * Float.pow (dx * dx + dy * dy + dz * dz) (-1.5)
      let b1m = b.mass * mag
      let b2m = o.mass * mag
      let b2 = { b | vx = b.vx - dx * b2m, vy = b.vy - dy * b2m, vz = b.vz - dz * b2m }
      let o2 = { o | vx = o.vx + dx * b1m, vy = o.vy + dy * b1m, vz = o.vz + dz * b1m }
      let (b3, more2) = interact dt b2 more
      (b3, o2 :: more2)

velocities : Float -> List Body -> <Panic> List Body
velocities dt bs =
  match bs with
    | [] -> []
    | b :: rest ->
      let (b2, rest2) = interact dt b rest
      b2 :: velocities dt rest2

move : Float -> Body -> <Panic> Body
move dt b = { b | x = b.x + dt * b.vx, y = b.y + dt * b.vy, z = b.z + dt * b.vz }

advance : Float -> Int -> List Body -> <Panic> List Body
advance dt n bs =
  if n == 0 then bs else advance dt (n - 1) (List.map (move dt) (velocities dt bs))

potential : Float -> Body -> List Body -> <Panic> Float
potential e b rest =
  match rest with
    | [] -> e
    | o :: more ->
      let dx = b.x - o.x
      let dy = b.y - o.y
      let dz = b.z - o.z
      potential (e - (b.mass * o.mass) / Float.pow (dx * dx + dy * dy + dz * dz) 0.5) b more

pairs : Float -> List Body -> <Panic> Float
pairs e bs =
  match bs with
    | [] -> e
    | b :: rest -> pairs (potential e b rest) rest

kinetic : Float -> List Body -> <Panic> Float
kinetic e bs =
  match bs with
    | [] -> e
    | b :: rest -> kinetic (e + b.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz) / 2.0) rest

energy : List Body -> <Panic> Float
energy bs = kinetic (pairs 0.0 bs) bs

main : Unit -> <IO, Panic> Unit
main () =
  let bs = offset_momentum (bodies ())
  println (Float.to_fixed 9 (energy bs))
  println (Float.to_fixed 9 (energy (advance 0.01 1000 bs)))
