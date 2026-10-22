-- 各段が `File` を持ったまま、いちばん深い所で `exit 3` を呼ぶ。巻き戻しは `Finish` まで届き、どの段の `File` も
-- 1回ずつ閉じる。
import Fs

deep : Int -> <IO, Panic> Int
deep n =
  let f = Fs.open "input.txt"
  let r = if n == 0 then exit 3 else deep (n - 1) + 1
  Fs.close f
  r

main : Unit -> <IO, Panic> Unit
main () =
  println "start"
  println (show (deep 3))
