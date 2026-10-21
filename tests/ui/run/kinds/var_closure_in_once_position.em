-- `var` が持つクロージャは `Unr` のままで、読む式で広がるので `once` の位置に渡せる。`var` を読む式も
-- 局所変数と同じく広げないと、`Unr` の変数を `Lin` の位置に渡すことになる (docs/spec/types.md の「値を読む式の矢印の線形性」)。
run_once : (once Unit -> String) -> String
run_once f = f ()

main : Unit -> <IO> Unit
main () =
  let s = "a" ++ "b"
  var f = fn () -> s
  println (run_once f)
  f <- fn () -> "c"
  println (run_once f)
