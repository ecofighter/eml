-- `once` の操作の継続 `k` を補助関数に渡し、渡した先で1回呼ぶ。
effect Ask where
  ask : Unit -> Int

apply_to : (Int -> <e> a) -> Int -> <e> a
apply_to f n = f n

main : Unit -> <IO> Unit
main () =
  let r =
    handle ask () + 1 with
      | ask () k -> apply_to k 41
  println (show_int r)
