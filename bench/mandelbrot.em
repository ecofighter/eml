-- mandelbrot (S14d の計測)。引数 (既定は 128) の大きさの正方形の格子で、集合に入る点の数を書く。Benchmarks Game の版は
-- PBM のバイト列を書くが、今の言語にはバイト列がないので数を書く。複素数は `Complex` の値で書く。今は `Complex` を作るたび
-- に箱を確保するか、reuse で手放す箱に書き直す。S14e の構造体の表現は、箱そのものをなくす
-- (docs/implementation/benchmarks.md の「S14d」)。
-- bench/numeric/mandelbrot.py は、同じ順で浮動小数の演算を行う Python の版である
data Complex =
  | Complex Float Float

add : Complex -> Complex -> <Panic> Complex
add (Complex a b) (Complex c d) = Complex (a + c) (b + d)

mul : Complex -> Complex -> <Panic> Complex
mul (Complex a b) (Complex c d) = Complex (a * c - b * d) (a * d + b * c)

norm2 : Complex -> <Panic> Float
norm2 (Complex a b) = a * a + b * b

-- `z ← z² + c` を最大50回まわし、絶対値の2乗が 4 を超えなければ集合に入る
inside : Complex -> <Panic> Bool
inside c =
  let go i z =
    if i == 50 then True
    else
      let next = add (mul z z) c
      if norm2 next > 4.0 then False else go (i + 1) next
  go 0 (Complex 0.0 0.0)

count : Int -> <Panic> Int
count n =
  let fs = Float.of_int n
  var found = 0
  for 0 n (fn y ->
    let ci = 2.0 * Float.of_int y / fs - 1.0
    for 0 n (fn x ->
      if inside (Complex (2.0 * Float.of_int x / fs - 1.5) ci) then found <- found + 1))
  found

main : Unit -> <IO, Panic> Unit
main () = println (show (count (size 128)))

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
usage () = panic "usage: mandelbrot [SIZE]"
