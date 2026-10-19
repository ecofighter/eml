-- 再開する操作と `never` の操作を持つエフェクトの handler を特殊化する。`never` の操作は節の関数の末尾呼び出しに
-- なり、その後ろは動かない。`never` の操作だけを起こす関数を末尾でなく呼ぶ経路では、handle を残す
effect Walk where
  step : Int -> Unit
  never stop : Unit -> a

walk : Int -> <Walk, Panic> Unit
walk i =
  if i == 3 then stop ()
  else
    step i
    walk (i + 1)

count : Int -> Int -> <Walk, Panic> Int
count i acc =
  if i > 4 then stop () else
    step i
    count (i + 1) (acc + i)

check : Int -> <Walk> Int
check i = if i == 3 then stop () else i

walk_checked : Int -> <Walk, Panic> Unit
walk_checked i =
  let j = check i
  step j
  walk_checked (i + 1)

main : Unit -> <IO, Panic> Unit
main () =
  handle walk 1 with
    | fn step n -> println "step \{n}"
    | ctl stop () -> println "stopped"
  let total = handle count 1 0 from 0 with
    | fn step n st -> ((), st + n)
    | ctl stop () st -> st * 10
    | return r st -> r + st
  println "total \{total}"
  handle walk_checked 1 with
    | fn step n -> println "checked \{n}"
    | ctl stop () -> println "stopped checked"
