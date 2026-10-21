-- 局所変数の関数は、何度か呼んだ後でも `once` の位置に渡せる。読む式の矢印は元の線形性以上の新しい Kind 変数に
-- なるので、`Unr` のクロージャを `Lin` の位置に置ける (docs/spec/types.md の「値を読む式の矢印の線形性」)。
run_once : (once Unit -> String) -> String
run_once f = f ()

main : Unit -> <IO> Unit
main () =
  let s = "hello" ++ " world"
  let g = fn () -> s
  println (g ())
  println (run_once g)
  println (g ())
