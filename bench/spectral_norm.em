-- spectral-norm。Benchmarks Game の行列 `A` の大きさを引数 (既定は 100) で受け、`v` に `Aᵀ A` を掛ける
-- ことを10回行って、スペクトルノルムを書く。ベクトルは要素が生の語の `UnboxedArray Float` なので、要素の
-- `Float` は箱に入らない (docs/implementation/benchmarks.md の「配列の `Float` の要素の箱」)。bench/numeric/spectral_norm.py は、
-- 同じ順で浮動小数の演算を行う Python の実装である
import Env
import Float
import String
import UnboxedArray

a : Int -> Int -> <Panic> Float
a i j = 1.0 / Float.of_int ((i + j) * (i + j + 1) / 2 + i + 1)

mul_av : UnboxedArray Float -> <Panic> UnboxedArray Float
mul_av v =
  let n = UnboxedArray.length v
  UnboxedArray.tabulate n (fn i ->
    var s = 0.0
    for 0 n (fn j -> s <- s + a i j * UnboxedArray.get v j)
    s)

mul_atv : UnboxedArray Float -> <Panic> UnboxedArray Float
mul_atv v =
  let n = UnboxedArray.length v
  UnboxedArray.tabulate n (fn i ->
    var s = 0.0
    for 0 n (fn j -> s <- s + a j i * UnboxedArray.get v j)
    s)

mul_atav : UnboxedArray Float -> <Panic> UnboxedArray Float
mul_atav v = mul_atv (mul_av v)

main : Unit -> <IO, Panic> Unit
main () =
  let n = size 100
  var u = UnboxedArray.make n 1.0
  var v = u
  for 0 10 (fn _ ->
    v <- mul_atav u
    u <- mul_atav v)
  var vbv = 0.0
  var vv = 0.0
  for 0 n (fn i ->
    vbv <- vbv + UnboxedArray.get u i * UnboxedArray.get v i
    vv <- vv + UnboxedArray.get v i * UnboxedArray.get v i)
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
