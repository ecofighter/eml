-- E3002: `once` の矢印の関数は1回だけ呼べる。フィールドから取り出した `once` の閉包を2回呼ぶと誤りになる。
data Job =
  | Job (once Unit -> Int)

run_twice : Job -> Int
run_twice job =
  match job with
    | Job f -> f () + f ()

main : Unit -> <IO> Unit
main () = println (show (run_twice (Job (fn () -> 1))))
