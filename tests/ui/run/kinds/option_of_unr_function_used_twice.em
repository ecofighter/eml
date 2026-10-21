-- `Some g` を束縛した `o` を2回使い、どちらも `Option (once ...)` の位置に渡せる。`o` を読むたびに
-- 引数の矢印が広がる (docs/spec/types.md の「値を読む式の矢印の線形性」)。
data Option a =
  | None
  | Some a

run_opt : Option (once Int -> Int) -> Int
run_opt o = match o with
  | Some f -> f 1
  | None -> 0

main : Unit -> <IO, Panic> Unit
main () =
  let g = fn x -> x
  let o = Some g
  let a = run_opt o
  let b = run_opt o
  println (show (a + b))
