-- 多相再帰の `keep_first` は一様 (`_`) な instance で動き、その中で作ったラムダは引数と結果を `tobj` で持つ。`main`
-- はそのラムダを `Int -> Int` や `Float -> Float` として型どおりの Repr で呼ぶので、`apply` が実行時に変換する
-- (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「`apply` の ABI」)。
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
