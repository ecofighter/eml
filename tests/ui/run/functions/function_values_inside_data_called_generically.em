-- 関数の値をリストに入れ、多相再帰の `compose_at` が一様 (`_`) な instance の中で取り出して呼ぶ。データに入った
-- 関数の値は静的には包めないので、型どおりの Repr のラムダを一様な場所から呼ぶ `apply` が実行時に変換する
-- (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「`apply` の ABI」)。
compose_at : Int -> List (a -> <Panic> a) -> a -> <Panic> a
compose_at n fs x =
  if n == 0 then List.fold (fn acc f -> f acc) x fs
  else
    let lifted = List.map (fn f -> fn p -> match p with | (y, u) -> (f y, u)) fs
    match compose_at (n - 1) lifted (x, ()) with
      | (y, _) -> y

main : Unit -> <IO, Panic> Unit
main () =
  println (show (compose_at 2 [fn n -> n + 1, fn n -> n * 3] 4))
  println (show (compose_at 1 [fn x -> x * 2.0, fn x -> x - 0.5] 1.25))
  println (compose_at 3 [fn s -> s ++ "a", fn s -> "b" ++ s] "-")
