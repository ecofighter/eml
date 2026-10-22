-- 配列の変数の範囲が `multi` の操作 `choose` をまたぐ。状態の配列は `Lin` なので、持ち越し規則が E3006 にする
import Array
import MutArray

effect Choose where
  multi choose : Unit -> Bool

pick : Unit -> <Choose, Panic> Int
pick () =
  var a = MutArray.make 2 0
  a[0] <- if choose () then 1 else 2
  Array.get (MutArray.freeze a) 0

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle pick () with
    | ctl choose () k -> k True + k False
  println (show r)
