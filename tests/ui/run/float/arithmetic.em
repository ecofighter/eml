-- `Float` の算術は IEEE 754 の倍精度に従う。0 で割ると無限大になり、`%` の結果は割られる数の符号を持つ。
-- `Show Float` はまだないので、結果をリテラルと `==` で比べる。
sum : List Float -> <Panic> Float
sum xs = match xs with
  | [] -> 0.0
  | x :: rest -> x + sum rest

main : Unit -> <IO, Panic> Unit
main () =
  println (show (0.1 + 0.2 == 0.30000000000000004))
  println (show (0.1 + 0.2 != 0.3))
  println (show (1.0 / 0.0 > 1e308))
  println (show (-1.5 * 2.0 == -3.0))
  println (show (7.5 % 2.0 == 1.5))
  println (show (-7.5 % 2.0 == -1.5))
  println (show (10.0 - 0.5 == 9.5))
  println (show (1_000.5 == 1000.5))
  println (show (2.5e-3 == 0.0025))
  println (show (1e3 == 1000.0))
  println (show (sum [0.5, 0.25, 0.125] == 0.875))
