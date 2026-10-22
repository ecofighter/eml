-- レコードを分解して手放した後、作り直す前に、埋め込まれない (自分を呼ぶ) 関数の IO の失敗が `try_io` へ巻き戻す。
-- 生きている token は巻き戻しで手放す。
-- フィールドを7つにして、Stats を構造体にせず箱のままにする (docs/spec/core-ir.md の「構造体にする型」)。
-- 残りの5つは使わない
import FileIn
import List

data Stats =
  | Stats { count : Int, total : Int, a : Int, b : Int, c : Int, d : Int, e : Int }

probe : Int -> Int -> <IO, Panic> Unit
probe depth count =
  if depth == 0 then (if count > 3 then FileIn.close (FileIn.open "missing.txt") else ())
  else probe (depth - 1) count

step : Stats -> <IO, Panic> Stats
step st =
  match st with
    | Stats { count, total } ->
      let next = count + 1
      probe 5 next
      Stats { count = next, total = total + next, a = 0, b = 0, c = 0, d = 0, e = 0 }

run : Int -> Stats -> <IO, Panic> Stats
run n st =
  if n == 0 then st
  else
    let st = step st
    println "count \{st.count}"
    run (n - 1) st

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> run (List.length [1, 2, 3, 4, 5, 6]) (Stats { count = 0, total = 0, a = 0, b = 0, c = 0, d = 0, e = 0 })) with
    | Ok st -> println "total \{st.total}"
    | Err e -> println "Err \{e}"
