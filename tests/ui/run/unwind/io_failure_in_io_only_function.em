-- `<IO>` だけの row の関数は yield しない。`FileIn` を持った段の下で無いファイルを開くと、IO の失敗が `try_io` へ
-- 中断し、巻き戻しが各段の `FileIn` を1回ずつ閉じる。
import FileIn

missing : Unit -> <IO> Int
missing () =
  let f = FileIn.open "input.txt"
  FileIn.close (FileIn.open "missing.txt")
  FileIn.close f
  0

outer : Unit -> <IO, Panic> Int
outer () =
  let f = FileIn.open "input.txt"
  let r = missing () + 1
  FileIn.close f
  r

main : Unit -> <IO, Panic> Unit
main () =
  match try_io outer with
    | Ok r -> println "ok \{r}"
    | Err e -> println "outer: \{e}"
