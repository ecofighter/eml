-- 各段が `File` を持ったまま、いちばん深い所で `exit 3` を呼ぶ。巻き戻しは `Finish` まで届き、どの段の `File` も
-- 1回ずつ閉じる。
deep : Int -> <IO> Int
deep n =
  let f = Fs.open "input.txt"
  let r = if n == 0 then exit 3 else deep (n - 1) + 1
  Fs.close f
  r

main : Unit -> <IO> Unit
main () =
  println "start"
  println (show (deep 3))
