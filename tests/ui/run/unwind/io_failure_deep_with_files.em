-- 各段が `File` を持ったまま、いちばん深い所で無いファイルを開き、IO の失敗が `try_io` へ中断する。どの段の
-- `File` も巻き戻しで1回ずつ閉じる。
missing : Unit -> <IO> Int
missing () =
  Fs.close (Fs.open "missing.txt")
  0

deep : Int -> <IO, Panic> Int
deep n =
  let f = Fs.open "input.txt"
  let r = if n == 0 then missing () else deep (n - 1) + 1
  Fs.close f
  r

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> deep 3) with
    | Ok r -> println "ok \{r}"
    | Err e -> println "deep: \{e}"
