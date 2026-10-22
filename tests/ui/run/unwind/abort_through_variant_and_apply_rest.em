-- `try_panic` が、一様な版のフレームと、一様な語を預けた `ApplyRest` のフレームの下で起きた `panic` を捕まえる。
-- `apply3` の適用 `g n 7 2.5` は、引数を1つ取る関数の値に3つの引数を渡すので、余った `7` と `2.5` を一様な語
-- (`2.5` は箱) にして `ApplyRest` のフレームに預ける。巻き戻しはフレームを外し、預けた語を参照として手放す。
-- `call_at` は型変数の位置が一様 (`_`) で、`thunk` はデータに通し、optimize が `call_at` を既知の閉包に特殊化しない
-- ようにする (docs/spec/lir.md の「関数の ABI」)。
import Float

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

data Box a = | Box a

hide : Int -> a -> <Panic> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x

thunk : Int -> Unit -> <Panic> (Unit -> <Panic> Int)
thunk a () =
  let Box f = hide 0 (fn () -> apply3 check a)
  f

main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> call_at 0 (thunk 1 ()))))
  println (show (try_panic (fn () -> call_at 0 (thunk 0 ()))))
