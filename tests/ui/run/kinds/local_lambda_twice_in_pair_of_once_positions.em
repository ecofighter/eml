-- 同じ局所のクロージャを、呼んだ後に `once` の位置が2つある組へ渡せる。読むたびに新しい Kind 変数になるので、
-- 2つの位置が互いの線形性を縛らない (docs/spec/types.md の「値を読む式の矢印の線形性」)。
pair : a -> a -> (a, a)
pair x y = (x, y)

run2 : (once Unit -> String, once Unit -> String) -> String
run2 p =
  match p with
    | (f, g) -> f () ++ g ()

main : Unit -> <IO> Unit
main () =
  let s = "a" ++ "b"
  let g = fn () -> s
  println (g ())
  println (run2 (pair g g))
