-- E3002: 読む式で広がるのは1段だけである。`Option (Option (once ...))` のように2段目より深い位置の矢印は広がらないので、
-- 2回使った値を渡すと誤りになる (docs/spec/types.md の「値を読む式の矢印の線形性」)。
run_all : Option (Option (once Int -> Int)) -> Int
run_all xs =
  match xs with
    | Some (Some f) -> f 1
    | Some None -> 0
    | None -> 0

main : Unit -> <IO> Unit
main () =
  let g = fn x -> x
  let xs = Some (Some g)
  println (show (run_all xs))
  println (show (run_all xs))
