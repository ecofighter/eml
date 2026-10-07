-- E2001: `once` の `k` は1回しか呼べない関数なので、矢印が `Unr` に決まる `data` のフィールドにはしまえない。
data Box =
  | Box (Int -> Box)
  | Done

effect Ask where
  ask : Unit -> Int

keep : Unit -> Box
keep () =
  handle ask () with
    | ask () k -> Box k
    | return x -> Done

main : Unit -> <IO> Unit
main () = println "x"
