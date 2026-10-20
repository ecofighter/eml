-- spectral-norm (S14d の計測)。Benchmarks Game の行列 `A` の大きさを引数 (既定は 100) で受け、`v` に `Aᵀ A` を掛ける
-- ことを10回行って、スペクトルノルムを書く。ベクトルは `Array Float` で、要素の `Float` は箱に入る
-- (docs/implementation/benchmarks.md の「S14d」)。bench/numeric/spectral_norm.py は、同じ順で浮動小数の演算を行う
-- Python の版である
a : Int -> Int -> <Panic> Float
a i j = 1.0 / Float.of_int ((i + j) * (i + j + 1) / 2 + i + 1)

mul_av : Array Float -> <Panic> Array Float
mul_av v =
  let n = Array.length v
  Array.tabulate n (fn i ->
    var s = 0.0
    for 0 n (fn j -> s <- s + a i j * v[j])
    s)

mul_atv : Array Float -> <Panic> Array Float
mul_atv v =
  let n = Array.length v
  Array.tabulate n (fn i ->
    var s = 0.0
    for 0 n (fn j -> s <- s + a j i * v[j])
    s)

mul_atav : Array Float -> <Panic> Array Float
mul_atav v = mul_atv (mul_av v)

main : Unit -> <IO, Panic> Unit
main () =
  let n = size 100
  var u = Array.make n 1.0
  var v = u
  for 0 10 (fn _ ->
    v <- mul_atav u
    u <- mul_atav v)
  var vbv = 0.0
  var vv = 0.0
  for 0 n (fn i ->
    vbv <- vbv + u[i] * v[i]
    vv <- vv + v[i] * v[i])
  println (Float.to_fixed 9 (Float.sqrt (vbv / vv)))

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
usage () = panic "usage: spectral_norm [SIZE]"
