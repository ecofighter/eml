-- 引数が2つの適用 `g 0 1` に、引数を1つずつ取る関数の値を渡す。適用は余った引数を `ApplyRest` の枠に預け、2つ目の
-- 呼び出しが外側の `Fail.to_result` へ中断する。呼び出し元のフレームが持つ `File` は閉じる。
import Fail (Fail(..))

check : Int -> Int -> <Fail String> Int
check = fn a -> fn b -> if a == 0 then Fail.fail "zero" else a + b

call2 : (Int -> Int -> <Fail String> Int) -> <Fail String, IO> Int
call2 g =
  let f = Fs.open "input.txt"
  let r = g 0 1
  Fs.close f
  r

main : Unit -> <IO> Unit
main () =
  println (show (Fail.to_result (fn () -> call2 check)))
