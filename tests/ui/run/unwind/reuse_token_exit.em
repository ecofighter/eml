-- レコードを分解して手放した後、作り直す前に、埋め込まれない (自分を呼ぶ) 関数が `exit` で中断する。生きている
-- token は巻き戻しで手放す。
data Stats = | Stats { count : Int, total : Int }

check : Int -> Int -> <IO> Unit
check depth count = if depth == 0 then (if count > 3 then exit 3 else ()) else check (depth - 1) count

step : Stats -> <IO> Stats
step st =
  match st with
    | Stats { count, total } ->
      let next = count + 1
      check 5 next
      Stats { count = next, total = total + next }

run : Int -> Stats -> <IO> Stats
run n st =
  if n == 0 then st
  else
    let st = step st
    println "count \{st.count}"
    run (n - 1) st

main : Unit -> <IO> Unit
main () =
  let st = run (List.length [1, 2, 3, 4, 5, 6]) (Stats { count = 0, total = 0 })
  println "total \{st.total}"
