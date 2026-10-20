-- `call_at` は多相再帰があるので、型変数の位置が一様 (`_`) である。その中で `Int` を返す関数の値を呼ぶと、`apply` は
-- 結果を箱に入れる変換のフレームを積んでから入る。呼んだ関数の中のゼロ除算の誤りは、変換のフレームでなく、その
-- 関数の中の位置を指す。関数の値はデータに通し、optimize が `call_at` を既知の閉包に特殊化しないようにする。
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
