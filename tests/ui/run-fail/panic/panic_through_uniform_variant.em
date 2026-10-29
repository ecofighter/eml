-- `call_at` は型変数の位置が一様 (`_`) である。その中で `divide` の部分適用を呼ぶと、`apply` は一様な変種 `divide$uniform`
-- に入り、変種が `divide` を呼ぶ (docs/spec/lir.md の「関数の ABI」)。ゼロ除算の誤りは、変種でなく `divide` の中の位置を
-- 指す。関数の値はデータに通し、optimize が `call_at` を既知の閉包に特殊化しないようにする。
divide : Int -> Unit -> <Panic> Int
divide d () = 100 / d

call_at : Int -> (Unit -> <Panic> a) -> <Panic> a
call_at n f =
  if n == 0 then f ()
  else
    match call_at (n - 1) (fn () -> (f (), ())) with
      | (x, _) -> x

data Box a = | Box a

hide : Int -> a -> <Panic> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x

main : Unit -> <IO, Panic> Unit
main () =
  let Box five = hide 0 (divide 5)
  println (show (call_at 0 five))
  let Box zero = hide 0 (divide 0)
  println (show (call_at 2 zero))
