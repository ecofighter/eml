-- レコードを分解して手放した後、作り直す前に、埋め込まれない (自分を呼ぶ) 関数の IO の失敗が `try_io` へ巻き戻す。
-- 生きている token は巻き戻しで手放す。
data Stats = | Stats { count : Int, total : Int }

probe : Int -> Int -> <IO, Panic> Unit
probe depth count =
  if depth == 0 then (if count > 3 then Fs.close (Fs.open "missing.txt") else ())
  else probe (depth - 1) count

step : Stats -> <IO, Panic> Stats
step st =
  match st with
    | Stats { count, total } ->
      let next = count + 1
      probe 5 next
      Stats { count = next, total = total + next }

run : Int -> Stats -> <IO, Panic> Stats
run n st =
  if n == 0 then st
  else
    let st = step st
    println "count \{st.count}"
    run (n - 1) st

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> run (List.length [1, 2, 3, 4, 5, 6]) (Stats { count = 0, total = 0 })) with
    | Ok st -> println "total \{st.total}"
    | Err e -> println "Err \{e}"
