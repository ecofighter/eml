-- `records.em` を7つの `Int` のフィールドのレコードにしたもの。構造体の表現は、フィールドが少ない
-- 値だけを対象にするので、このレコードは構造体にならず、箱で更新し続ける。`reuses` が 0 より大きいことで、
-- 構造体にしない値の reuse が残ることを確かめる (docs/implementation/benchmarks.md の「基準のプログラム」)
data Stats =
  | Stats { count : Int, total : Int, max : Int, last : Int, sum_sq : Int, odd : Int, prev : Int }

update : Stats -> Int -> <Panic> Stats
update st x = { st | count = st.count + 1, total = st.total + x, max = if x > st.max then x else st.max, last = x, sum_sq = st.sum_sq + x * x, odd = st.odd + x % 2, prev = st.last }

go : Int -> Int -> Stats -> <Panic> Stats
go n seed st = if n == 0 then st else
  let next = (seed * 1103515245 + 12345) % 2147483648
  go (n - 1) next (update st (next % 1000))

main : Unit -> <IO, Panic> Unit
main () =
  let st = go 100000 7 (Stats { count = 0, total = 0, max = 0, last = 0, sum_sq = 0, odd = 0, prev = 0 })
  println "\{st.count} \{st.total} \{st.max} \{st.last} \{st.sum_sq} \{st.odd} \{st.prev}"
