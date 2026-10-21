-- `Option` のような共変の型構成子の引数も、読む式で1段だけ広がる。局所のクロージャを `Some` に入れて
-- `Option (once ...)` の位置に渡せる (docs/spec/types.md の「値を読む式の矢印の線形性」)。
run_opt : Option (once Unit -> String) -> String
run_opt o =
  match o with
    | Some f -> f ()
    | None -> "none"

main : Unit -> <IO> Unit
main () =
  let s = "a" ++ "b"
  let g = fn () -> s
  println (g ())
  println (run_opt (Some g))
