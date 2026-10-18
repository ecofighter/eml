-- 4つのフィールドのレコードを `{ st | … }` で更新し続ける。古いレコードは一意なので、更新で作るレコードはその箱を
-- 使い回す (docs/spec/lir.md の「reuse」)。Perceus は `decref st` を出すので、`decref` を reuse の元にする規則が効く。
data Stats =
  | Stats { count : Int, total : Int, max : Int, last : Int }

update : Stats -> Int -> Stats
update st x = { st | count = st.count + 1, total = st.total + x, max = if x > st.max then x else st.max, last = x }

go : Int -> Int -> Stats -> Stats
go n seed st = if n == 0 then st else
  let next = (seed * 1103515245 + 12345) % 2147483648
  go (n - 1) next (update st (next % 1000))

main : Unit -> <IO> Unit
main () =
  let st = go 100000 7 (Stats { count = 0, total = 0, max = 0, last = 0 })
  println "\{st.count} \{st.total} \{st.max} \{st.last}"
