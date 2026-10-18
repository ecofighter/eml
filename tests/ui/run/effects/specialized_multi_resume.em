-- `multi` の操作の節が `k` を2回呼ぶ handler を特殊化する。末尾再帰で選ぶ関数の続きは、継続の関数の閉包になり、
-- 2回呼んでも捕獲を写すだけである
effect Choice where
  multi choose : Unit -> Bool

pick : Int -> String -> <Choice> String
pick n acc =
  if n == 0 then acc
  else
    let b = choose ()
    pick (n - 1) (acc ++ (if b then "1" else "0"))

main : Unit -> <IO> Unit
main () =
  let all = handle pick 3 "" with
    | ctl choose () k -> k True ++ k False
    | return s -> "[" ++ s ++ "]"
  println all
