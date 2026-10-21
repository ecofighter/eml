-- 多相再帰の `keep_first` は一様 (`_`) な instance で動き、その中でラムダを作る。`main` はそのラムダを `Int -> Int` や
-- `Float -> Float` として呼ぶ。関数の値は捕獲より後ろの引数と結果がいつも一様なので、作った所と呼ぶ所が違う型を
-- 見ていても、`apply` は引数と結果を箱に入れて受け渡すだけでよい (docs/spec/lir.md の「関数の ABI」)。
keep_first : Int -> a -> <Panic> (a -> <Panic> a)
keep_first n x =
  if n == 0 then fn y -> if n == 0 then x else y
  else
    let g = keep_first (n - 1) (x, x)
    fn y -> match g (y, y) with | (z, _) -> z

main : Unit -> <IO, Panic> Unit
main () =
  let f = keep_first 3 5
  println (show (f 7))
  let g = keep_first 2 1.5
  println (show (g 2.5))
  let h = keep_first 1 "first"
  println (h "second")
