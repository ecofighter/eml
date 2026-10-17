-- 各段が `File` を持ったまま、いちばん深い所で `fail` して外側の `Fail.to_result` へ中断する。どの段の `File` も
-- 巻き戻しで1回ずつ閉じる。
import Fail (Fail(..))

deep : Int -> <Fail String, IO> Int
deep n =
  let f = Fs.open "input.txt"
  let r = if n == 0 then Fail.fail "stop" else deep (n - 1) + 1
  Fs.close f
  r

main : Unit -> <IO> Unit
main () =
  println (show (Fail.to_result (fn () -> deep 3)))
