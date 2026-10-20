-- `try_panic` が、変換のフレームと、生の `Int` と `Float` を預けた `ApplyRest` のフレームの下で起きた `panic` を
-- 捕まえる。`call_at` は型変数の位置が一様 (`_`) なので、`Int` を返す `thunk` を呼ぶ `apply` は変換のフレームを積む。
-- `apply3` の適用 `g n 7 2.5` は、引数を1つ取る関数の値に3つの引数を渡すので、余った `7` と `2.5` を `ApplyRest` の
-- フレームに預ける。巻き戻しは2つのフレームを外し、どちらの生の語も参照として手放さない。
check : Int -> <Panic> (Int -> Float -> <Panic> Int)
check = fn a -> if a == 0 then panic "zero" else fn b c -> a + b + Float.truncate c

apply3 : (Int -> <Panic> (Int -> Float -> <Panic> Int)) -> Int -> <Panic> Int
apply3 g n = g n 7 2.5

call_at : Int -> (Unit -> <Panic> a) -> <Panic> a
call_at n f =
  if n == 0 then f ()
  else
    match call_at (n - 1) (fn () -> (f (), ())) with
      | (x, _) -> x

main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> call_at 0 (fn () -> apply3 check 1))))
  println (show (try_panic (fn () -> call_at 0 (fn () -> apply3 check 0))))
