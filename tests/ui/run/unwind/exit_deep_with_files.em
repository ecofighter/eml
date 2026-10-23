-- 各段が `FileIn` を持ったまま、いちばん深い所で `exit 3` を呼ぶ。巻き戻しは `Finish` まで届き、どの段の `FileIn` も
-- 1回ずつ閉じる。
import FileIn

deep : Int -> <IO, Panic> Int
deep n =
  let f = FileIn.open "input.txt"
  let r = if n == 0 then exit 3 else deep (n - 1) + 1
  FileIn.close f
  r

main : Unit -> <IO, Panic> Unit
main () =
  println "start"
  println (show (deep 3))
